#!/bin/bash
echo "╔══════════════════════════════════╗"
echo "║      SEL Agent Health Check      ║"
echo "╚══════════════════════════════════╝"

# 1. Internet
echo -n "🌐 Internet:     "
if curl -s --connect-timeout 5 https://google.com > /dev/null; then
    echo "✅ Connected"
else
    echo "❌ No connection"
fi

# 2. Groq API reachable
echo -n "🔌 Groq Server:  "
if curl -s --connect-timeout 5 https://api.groq.com > /dev/null; then
    echo "✅ Reachable"
else
    echo "❌ Unreachable"
fi

# 3. API Key
echo -n "🔑 API Key:      "
if [ -z "$GROQ_API_KEY" ]; then
    echo "❌ Not set"
else
    echo "✅ Set (${GROQ_API_KEY:0:8}...)"
fi

# 4. API Key valid + Model
echo -n "🤖 Model:        "
response=$(curl -s --connect-timeout 10 \
  https://api.groq.com/openai/v1/chat/completions \
  -H "Authorization: Bearer $GROQ_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama-3.3-70b-versatile","messages":[{"role":"user","content":"reply: OK"}],"max_tokens":5}')

if echo "$response" | grep -q '"OK"'; then
    echo "✅ llama-3.3-70b responding"
elif echo "$response" | grep -q "invalid_api_key"; then
    echo "❌ Invalid API key"
elif echo "$response" | grep -q "rate_limit"; then
    echo "⚠️  Rate limited — wait 1 min"
elif echo "$response" | grep -q "model_not_found"; then
    echo "❌ Model not found"
else
    model=$(echo "$response" | grep -o '"model":"[^"]*"' | head -1)
    if [ -n "$model" ]; then
        echo "✅ $model"
    else
        echo "⚠️  Unknown: ${response:0:80}"
    fi
fi

# 5. SEL binary
echo -n "⚙️  SEL Binary:   "
if [ -f "./target/release/sel-agent" ]; then
    echo "✅ Built"
else
    echo "❌ Not built — run: cargo build --release"
fi

echo "══════════════════════════════════"
