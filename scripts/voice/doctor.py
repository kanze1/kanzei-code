"""Check the inference compiler before loading large model weights."""
from cuda_runtime import configure_cuda

configure_cuda()
import torch
from flashinfer.sampling import top_k_top_p_sampling_from_probs
assert torch.cuda.is_available(), "CUDA is not available"
probabilities = torch.softmax(torch.rand((1, 128), device="cuda"), dim=-1)
result = top_k_top_p_sampling_from_probs(probabilities, 20, .9)
torch.cuda.synchronize()
print(f"CUDA sampling passed: torch={torch.__version__}, GPU={torch.cuda.get_device_name()}, shape={tuple(result.shape)}")
