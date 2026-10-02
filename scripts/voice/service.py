"""Kanzei loopback voice gateway: local Whisper ASR and streaming Qwen3-TTS.

No microphone, cloud fallback, audio logging or model downloads happen here.
The desktop owns microphone permission, turn ownership and physical playback.
"""
import argparse
import asyncio
from contextlib import asynccontextmanager
import io
import json
from pathlib import Path
import threading
import wave
import base64
from urllib.parse import urlparse

import httpx
import numpy as np
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import StreamingResponse
from pydantic import BaseModel, Field


def read_pcm_wav(data: bytes) -> np.ndarray:
    try:
        with wave.open(io.BytesIO(data)) as audio:
            if audio.getnchannels() != 1 or audio.getsampwidth() != 2 or audio.getframerate() != 16000:
                raise ValueError("Expected 16kHz mono PCM16 WAV")
            frames = audio.getnframes()
            if not 1600 <= frames <= 16000 * 30:
                raise ValueError("Recording must be 0.1–30 seconds")
            pcm = audio.readframes(frames)
            if len(pcm) != frames * 2:
                raise ValueError("Truncated recording")
    except (wave.Error, EOFError) as error:
        raise ValueError("Invalid WAV recording") from error
    return np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0


class Speech(BaseModel):
    text: str = Field(min_length=1, max_length=600)
    language: str = "auto"


def create_app(config: dict, recognizer=None):
    target = urlparse(config["tts_url"])
    if target.scheme != "http" or target.hostname not in ("127.0.0.1", "localhost", "::1") or target.username:
        raise ValueError("TTS must use a loopback HTTP endpoint")
    reference = Path(config["reference_audio"])
    reference_url = "data:audio/wav;base64," + base64.b64encode(reference.read_bytes()).decode("ascii")
    lock = threading.Lock()
    engine = recognizer

    @asynccontextmanager
    async def lifetime(app):
        nonlocal engine
        if engine is None:
            from faster_whisper import WhisperModel
            engine = await asyncio.to_thread(WhisperModel, config["asr_model"], device="cpu", compute_type="int8", cpu_threads=4)
        yield

    app = FastAPI(lifespan=lifetime, docs_url=None, redoc_url=None, openapi_url=None)

    @app.get("/health")
    async def health():
        tts_ready = False
        detail = ""
        try:
            async with httpx.AsyncClient(trust_env=False, timeout=3) as client:
                response = await client.get(config["tts_url"] + "/v1/models")
                response.raise_for_status()
                tts_ready = any(model.get("id") == config["tts_model"] for model in response.json().get("data", []))
                if not tts_ready:
                    detail = "TTS model does not match the selected voice model"
        except (httpx.HTTPError, ValueError) as error:
            detail = f"TTS unavailable: {type(error).__name__}"
        return {"ready": engine is not None and tts_ready, "asrReady": engine is not None, "ttsReady": tts_ready,
                "voice": "Kanzei A75", "sampleRate": 24000, "detail": detail}

    @app.post("/transcribe")
    async def transcribe(request: Request, language: str = "auto"):
        if language not in ("auto", "zh", "en", "ja"):
            raise HTTPException(422, "Unsupported recognition language")
        data = bytearray()
        async for chunk in request.stream():
            data.extend(chunk)
            if len(data) > 960_128:
                raise HTTPException(413, "Recording exceeds 30 seconds")
        try:
            audio = read_pcm_wav(bytes(data))
        except ValueError as error:
            raise HTTPException(422, str(error)) from error
        if engine is None:
            raise HTTPException(503, "ASR is not ready")

        def recognize():
            with lock:
                segments, info = engine.transcribe(audio, language=None if language == "auto" else language,
                    beam_size=3, vad_filter=True, condition_on_previous_text=False,
                    vad_parameters={"min_silence_duration_ms": 350})
                text = "".join(segment.text for segment in segments if segment.no_speech_prob < .7).strip()
                return {"text": text, "language": info.language}
        return await asyncio.to_thread(recognize)

    @app.post("/speak")
    async def speak(speech: Speech):
        language = {"auto": "Auto", "zh": "Chinese", "en": "English", "ja": "Japanese"}.get(speech.language)
        if language is None or not speech.text.strip():
            raise HTTPException(422, "Invalid speech request")
        client = httpx.AsyncClient(trust_env=False, timeout=httpx.Timeout(90, connect=3))
        payload = {"model": config["tts_model"], "input": speech.text, "task_type": "Base", "voice": "default",
            "language": language, "ref_audio": reference_url, "x_vector_only_mode": True,
            "response_format": "pcm", "stream": True, "stream_format": "audio", "sample_rate": 24000}
        try:
            upstream = await client.send(client.build_request("POST", config["tts_url"] + "/v1/audio/speech", json=payload), stream=True)
            if not upstream.is_success:
                await upstream.aclose()
                raise HTTPException(502, f"TTS rejected the request (HTTP {upstream.status_code})")
        except BaseException:
            await client.aclose()
            raise

        async def chunks():
            try:
                async for chunk in upstream.aiter_bytes():
                    yield chunk
            finally:
                # Client disconnect closes the producer request, including on barge-in.
                await upstream.aclose()
                await client.aclose()
        return StreamingResponse(chunks(), media_type="application/octet-stream",
            headers={"X-Audio-Format": "pcm-s16le", "X-Sample-Rate": "24000", "Cache-Control": "no-store"})

    return app


if __name__ == "__main__":
    import uvicorn
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--port", type=int, default=7388)
    args = parser.parse_args()
    settings = json.loads(args.config.read_text(encoding="utf-8-sig"))
    uvicorn.run(create_app(settings), host="127.0.0.1", port=args.port, access_log=False)
