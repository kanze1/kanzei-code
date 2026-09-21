"""Make NVIDIA's pip toolkit usable by JIT compilers without a system CUDA install."""
import importlib.util
import os
from pathlib import Path
import sys


def configure_cuda(environment=None):
    environment = os.environ if environment is None else environment
    package = Path(importlib.util.find_spec("vllm_omni").origin).parent
    cuda = package.parent / "nvidia/cu13"
    if not (cuda / "bin/nvcc").is_file():
        raise RuntimeError("The private voice environment is missing CUDA nvcc")
    # The pip distribution has lib/ and only a versioned cudart shared object.
    # Keep linker aliases inside this venv; never modify the system toolkit.
    aliases = Path(sys.prefix) / "kanzei-cuda-libs"
    aliases.mkdir(exist_ok=True)
    cudart = aliases / "libcudart.so"
    target = cuda / "lib/libcudart.so.13"
    if not target.is_file():
        raise RuntimeError("The private voice environment is missing libcudart.so.13")
    if not cudart.exists():
        cudart.symlink_to(target)
    libraries = [str(aliases), str(cuda / "lib")]
    if Path("/usr/lib/wsl/lib/libcuda.so").is_file():
        libraries.append("/usr/lib/wsl/lib")
    environment["CUDA_HOME"] = str(cuda)
    environment["VIRTUAL_ENV"] = sys.prefix
    environment["PATH"] = os.pathsep.join([
        str(Path(sys.executable).parent), str(cuda / "bin"), environment.get("PATH", "")
    ])
    for variable in ("LIBRARY_PATH", "LD_LIBRARY_PATH"):
        environment[variable] = os.pathsep.join(libraries + [environment.get(variable, "")])
    return environment
