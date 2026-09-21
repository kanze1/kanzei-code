import io
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
import wave

from fastapi.testclient import TestClient
from service import create_app, read_pcm_wav


def recording(seconds=1, rate=16000, channels=1):
    output = io.BytesIO()
    with wave.open(output, "wb") as audio:
        audio.setnchannels(channels); audio.setsampwidth(2); audio.setframerate(rate)
        audio.writeframes(b"\0\0" * int(seconds * rate) * channels)
    return output.getvalue()


class VoiceServiceTests(unittest.TestCase):
    def test_wav_bounds_and_format(self):
        self.assertEqual(len(read_pcm_wav(recording())), 16000)
        for data in [b"invalid", recording(rate=24000), recording(channels=2), recording(seconds=31), recording()[:-10]]:
            with self.assertRaises(ValueError):
                read_pcm_wav(data)

    def test_transcription_contract_rejects_invalid_input(self):
        class Recognizer:
            def transcribe(self, audio, **options):
                self.options = options
                return iter([SimpleNamespace(text=" hello", no_speech_prob=.02), SimpleNamespace(text=" false speech", no_speech_prob=.95)]), SimpleNamespace(language="en")
        recognizer = Recognizer()
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "reference.wav"; path.write_bytes(recording())
            config = {"tts_url":"http://127.0.0.1:8091", "reference_audio":str(path), "tts_model":"kanzei-a75"}
            with TestClient(create_app(config, recognizer)) as client:
                response = client.post("/transcribe?language=en", content=recording())
                self.assertEqual(response.status_code, 200)
                self.assertEqual(response.json(), {"text":"hello", "language":"en"})
                self.assertTrue(recognizer.options["vad_filter"])
                self.assertFalse(recognizer.options["condition_on_previous_text"])
                self.assertEqual(client.post("/transcribe", content=b"bad").status_code, 422)
                self.assertEqual(client.post("/transcribe", content=b"0" * 1_000_000).status_code, 413)
                self.assertEqual(client.post("/speak", json={"text":"hello", "language":"invalid"}).status_code, 422)
            with self.assertRaises(ValueError):
                create_app({**config, "tts_url":"https://example.com"}, recognizer)


if __name__ == "__main__":
    unittest.main()
