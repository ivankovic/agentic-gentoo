"""What the drills' runner and tools/fix's fix-shadow both do with a chat server: clip a long
text for the prompt, and post a chat request. One copy, so the two put the same thing before the
model. fix-shadow is installed with this file beside it (the overlay's app-admin/fix)."""
import json
import urllib.request


def clip(text, limit):
    """TEXT, or its head and tail with a line between them saying how much is left out."""
    if len(text) <= limit:
        return text
    half = limit // 2
    return f"{text[:half]}\n[... {len(text) - limit} characters left out ...]\n{text[-half:]}"


def post(url, body, timeout=600):
    """The server's answer to one chat request (OpenAI's /v1/chat/completions), parsed. What
    urllib raises is the caller's to read: each has its own meaning for an error."""
    req = urllib.request.Request(url.rstrip("/") + "/v1/chat/completions", json.dumps(body).encode(),
                                 {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)
