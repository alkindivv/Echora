#!/usr/bin/env python3
"""Deterministic Responses stream for native/reference live conversation checks."""

import argparse
import http.server
import json
from pathlib import Path
import time


REPLY = """## Streaming verification

Plain text stays in the same place while **bold text**, *emphasis*, `inline_code`, and a [link](https://example.com) arrive.

- First item
- Second item with **strong text**

1. Ordered item
2. Another item

> A quote with a second line of text.

```rust
fn main() {
    println!("hello");
}
```

| State | Result |
| --- | --- |
| Streaming | In progress |
| Complete | Ready |

The final paragraph ends here.
"""


class Responses(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))))
        inputs = body.get("input", [])
        text = json.dumps(inputs, ensure_ascii=False)
        identifier = str(time.time_ns())
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()

        def event(kind, **payload):
            data = json.dumps({"type": kind, **payload})
            self.wfile.write(f"event: {kind}\ndata: {data}\n\n".encode())
            self.wfile.flush()

        try:
            event("response.created", response={"id": identifier})
            time.sleep(0.8)
            if "FAIL_STREAM" in text:
                event("error", code="context_length_exceeded", message="Fixture failure")
                return
            tool_done = any(item.get("type") == "function_call_output" for item in inputs)
            if "TOOLS_STREAM" in text and not tool_done:
                reasoning = {"type": "reasoning", "id": "reason-" + identifier, "summary": []}
                event("response.output_item.added", output_index=0, item=reasoning)
                event("response.reasoning_summary_part.added", output_index=0, item_id=reasoning["id"], summary_index=0, part={"type": "summary_text", "text": ""})
                summary = "**Checking the fixture**\n\nReading the command output."
                for offset in range(0, len(summary), 5):
                    event("response.reasoning_summary_text.delta", output_index=0, item_id=reasoning["id"], summary_index=0, delta=summary[offset:offset + 5])
                    time.sleep(0.12)
                event("response.output_item.done", output_index=0, item={**reasoning, "summary": [{"type": "summary_text", "text": summary}]})
                command = {"type": "function_call", "id": "fc-" + identifier, "call_id": "call-" + identifier, "name": "exec_command", "arguments": json.dumps({"cmd": "printf 'fixture output\\n'; sleep 3", "yield_time_ms": 10000})}
                event("response.output_item.added", output_index=1, item={**command, "arguments": ""})
                event("response.function_call_arguments.delta", output_index=1, item_id=command["id"], delta=command["arguments"])
                event("response.output_item.done", output_index=1, item=command)
            else:
                reply = REPLY if "STREAM" in text else "Fixture chat"
                if "PLAIN_STREAM" in text:
                    reply = "A plain reply arrives a few characters at a time.\n\nThe second paragraph stays aligned with the first."
                message = {"type": "message", "role": "assistant", "id": "msg-" + identifier, "phase": "final_answer", "content": []}
                event("response.output_item.added", output_index=0, item=message)
                event("response.content_part.added", output_index=0, item_id=message["id"], content_index=0, part={"type": "output_text", "text": "", "annotations": []})
                for offset in range(0, len(reply), 5):
                    event("response.output_text.delta", output_index=0, item_id=message["id"], content_index=0, delta=reply[offset:offset + 5])
                    time.sleep(0.08)
                    if "FAIL_AFTER_STREAM" in text and offset >= 100:
                        event("error", code="context_length_exceeded", message="Fixture failure after partial output")
                        return
                event("response.output_item.done", output_index=0, item={**message, "content": [{"type": "output_text", "text": reply, "annotations": []}]})
            event("response.completed", response={"id": identifier, "usage": {"input_tokens": 10, "output_tokens": 10, "total_tokens": 20, "input_tokens_details": {"cached_tokens": 0}, "output_tokens_details": {"reasoning_tokens": 0}}})
        except (BrokenPipeError, ConnectionResetError):
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=9474)
    parser.add_argument("--init-home", type=Path)
    args = parser.parse_args()
    if args.init_home:
        home = args.init_home.expanduser().resolve()
        if home == Path.home() / ".codex":
            parser.error("use an isolated CODEX_HOME")
        home.mkdir(parents=True, exist_ok=True)
        config = home / "config.toml"
        backup = home / "config.before-live-fixture.toml"
        if config.exists() and not backup.exists():
            config.rename(backup)
        config.write_text(f'''model = "gpt-5.5"
model_provider = "live_fixture"
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "medium"
[model_providers.live_fixture]
name = "Live fixture"
base_url = "http://127.0.0.1:{args.port}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
''')
        return
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Responses)
    print(f"live fixture listening on {args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
