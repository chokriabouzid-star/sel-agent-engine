# Supported Models

## 🆓 Free Models

### Gemini (Google) - **RECOMMENDED FOR FREE TIER**
```bash
export GEMINI_API_KEY=AIza...
export GEMINI_MODEL=gemini-2.5-flash-lite  # Default
```
- ✅ Free tier: 15 RPM, 1M TPM
- ✅ Fast & accurate
- ✅ Good for complex tasks
- 🎯 Best for: General purpose, FastAPI, REST APIs

### Groq (Fast inference)
```bash
export GROQ_API_KEY=gsk_...
export SEL_MODEL=llama-3.3-70b-versatile  # Default
```
- ⚡ Ultra-fast (500 tokens/s)
- ⚠️ Daily TPM limits (14,400/day for Kimi)
- 🎯 Best for: Quick prototypes

### OpenRouter (Aggregator)
```bash
export OPENROUTER_API_KEY=sk-or-v1-...
export SEL_MODEL=deepseek/deepseek-chat-v3-0324
```
- 💰 $1 free credit/month (~2M tokens)
- 🎯 DeepSeek V3: Excellent for coding
- 🎯 Qwen Coder: Specialized for code generation

## Priority Order
1. `GEMINI_API_KEY` → Gemini API (recommended)
2. `SEL_API_KEY` → Custom endpoint
3. `GROQ_API_KEY` → Groq
4. `OPENROUTER_API_KEY` → OpenRouter

## Quick Start

### Default (Gemini)
```bash
export GEMINI_API_KEY=AIza...
sel-agent run --workspace /tmp/proj --goal "create Flask hello world"
```

### DeepSeek (best for complex code)
```bash
export OPENROUTER_API_KEY=sk-or-v1-...
export SEL_MODEL=deepseek/deepseek-chat-v3-0324
sel-agent run --workspace /tmp/api --goal "create REST API with JWT auth"
```

### Groq (fastest)
```bash
export GROQ_API_KEY=gsk_...
sel-agent run --workspace /tmp/quick --goal "python fizzbuzz"
```

## Get API Keys
- Gemini: https://aistudio.google.com/apikey (free, no credit card)
- Groq: https://console.groq.com/keys (free, requires email)
- OpenRouter: https://openrouter.ai/settings/keys (free $1/month)
