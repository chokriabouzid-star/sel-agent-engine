import urllib.request
import json

url = 'https://api.cerebras.ai/v1/chat/completions'
headers = {
    'Authorization': 'Bearer csk-crcxjfh89mkd28k9eh4dw4nd4jntvfx52wvxnxfxe9tn83y4',
    'Content-Type': 'application/json',
    'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
}
data = json.dumps({
    'model': 'qwen-3-235b-a22b-instruct-2507',
    'messages': [{'role': 'user', 'content': 'Hello ' * 6000}],
    'max_tokens': 4000
}).encode('utf-8')

req = urllib.request.Request(url, data=data, headers=headers)
try:
    with urllib.request.urlopen(req) as f:
        print("SUCCESS:", f.read().decode('utf-8'))
except Exception as e:
    print('ERROR:', e)
    if hasattr(e, 'read'):
        print('BODY:', e.read().decode('utf-8'))
