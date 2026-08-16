"""Double Love ASR sidecar 包。

JSONL 行协议（stdin 命令 / stdout 事件），引擎二选一：
- mock        —— 确定性假数据，测试与开发自举（DOUBLELOVE_ASR_MOCK=1）
- transcriber —— mlx-qwen3-asr（Qwen3-ASR + ForcedAligner 逐词时间戳）
"""
