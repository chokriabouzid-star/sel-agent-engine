import urllib.request
import json
import os

url = 'https://api.groq.com/openai/v1/chat/completions'
headers = {
    'Authorization': 'Bearer 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',
    'Content-Type': 'application/json',
    'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
}
data = json.dumps({
    'model': 'llama-3.3-70b-versatile',
    'messages': [{'role': 'user', 'content': 'Hello'}],
    'max_tokens': 10
}).encode('utf-8')

req = urllib.request.Request(url, data=data, headers=headers)
try:
    with urllib.request.urlopen(req) as f:
        print("SUCCESS:", f.read().decode('utf-8'))
except Exception as e:
    print('ERROR:', e)
    if hasattr(e, 'read'):
        print('BODY:', e.read().decode('utf-8'))
