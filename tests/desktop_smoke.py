#!/usr/bin/env python3
"""Read-only Windows/Linux session smoke test; saves evidence, never injects input."""
import argparse
import json
import pathlib
import queue
import subprocess
import tempfile
import threading


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/debug/unimation")
    parser.add_argument("--pid", type=int, help="Optional application accessibility scope")
    parser.add_argument("--capture", action="store_true", help="Also save a desktop screenshot")
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    output = args.output or pathlib.Path(tempfile.mkdtemp(prefix="unimation-desktop-"))
    output.mkdir(parents=True, exist_ok=True)
    replies = queue.Queue()
    transcript = []
    sequence = 0
    failures = []
    with (output / "stderr.log").open("w", encoding="utf-8") as stderr:
        process = subprocess.Popen(
            [args.binary, "--provider", "native", "session", "--json"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8", bufsize=1,
        )

        def reader():
            for line in process.stdout:
                replies.put(line)
            replies.put(None)

        threading.Thread(target=reader, daemon=True).start()

        def call(op, **fields):
            nonlocal sequence
            sequence += 1
            request = {"id": sequence, "op": op, **fields}
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            try:
                line = replies.get(timeout=args.timeout)
            except queue.Empty:
                raise RuntimeError(f"{op}: reply timeout; see stderr.log") from None
            if line is None:
                raise RuntimeError(f"{op}: session exited; see stderr.log")
            reply = json.loads(line)
            transcript.append({"request": request, "reply": reply})
            if reply.get("id") != sequence:
                raise RuntimeError(f"{op}: reply ID mismatch")
            if ("result" in reply) == ("error" in reply):
                raise RuntimeError(f"{op}: expected exactly one result or error")
            if "error" in reply:
                failures.append({"operation": op, "error": reply["error"]})
                print(f"{op}: {reply['error']}")
                return None
            print(f"{op}: received result; native correctness still requires inspection")
            return reply["result"]

        try:
            for op in ["capabilities", "discover", "windows", "displays"]:
                call(op)
            if args.pid is not None:
                request = {"pid": args.pid, "max_nodes": 250, "max_depth": 12}
                before = call("snapshot", request=request)
                after = call("snapshot", request=request)
                for name, snapshot in [("before", before), ("after", after)]:
                    if snapshot is not None:
                        (output / f"{name}.json").write_text(json.dumps(snapshot, indent=2), encoding="utf-8")
                if before is not None and after is not None:
                    assert before["root"]["session"] == after["root"]["session"], "namespace changed within session"
                    assert after["revision"] > before["revision"], "revision did not advance"
                    # Root identity may change if the application itself replaced it.
                    call("diff", before=before["revision"], after=after["revision"])
            if args.capture:
                call("capture", path=str((output / "desktop.png").resolve()))
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            (output / "transcript.json").write_text(json.dumps(transcript, indent=2), encoding="utf-8")
            print(f"Evidence: {output.resolve()}")
    if process.returncode != 0 or failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
