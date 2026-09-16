# Buffered reads after remote close

Source: https://github.com/poapoauu/DroidMux at d21e3d729a9d233fd34ce27e5c2244c45c4c9a8e. MIT OR Apache-2.0 licenses retained.

The upstream router already keeps acknowledgement records after remote CLSE. This patch additionally preserves an already-dequeued payload when its ACK reports StreamClosed and the stream's observed state is an orderly RemoteClosed. Local cancellation, SessionClosed transport errors and protocol violations remain errors.

Invalid stream IDs or inbound flow-control violations previously used the same RemoteClosed state as orderly peer closure. They now set an error state so the buffered-read handling cannot treat those violations as normal EOF.

Existing deterministic tests queue final WRTE+CLSE before reading, drain multiple burst packets after CLSE, and preserve sibling stream usability. Added unit cases cover a retired ACK with buffered data and reject local/session/protocol failures.

## Legacy receive bursts

A Samsung Android 16 device sent stdout and the six-byte Shell v2 exit frame in
separate WRTE packets before receiving an ACK. Rejecting the second packet lost
valid output. AOSP's receiver also queues WRTE packets rather than rejecting a
second pending packet: `adb.cpp` dispatches A_WRTE to `local_socket_enqueue`, and
`sockets.cpp` applies queue/flush backpressure.

Sources: https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/adb.cpp
and https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/sockets.cpp

The adapter now buffers bounded legacy bursts and acknowledges every consumed
packet. The configured per-stream byte limit also applies to legacy mode;
a 4096-packet ceiling prevents empty/tiny packets from bypassing memory bounds.
Malformed IDs, excessive buffered data, session loss and cancellations still
fail explicitly. Tests reproduce stdout+exit+CLSE before consumption, reject
byte-limit overflow and reject empty-packet floods.

`ACTUATE_ADB_TRACE_METADATA` enables stderr packet direction/type, stream IDs
and length diagnostics. It never prints shell commands, service names or payload
contents. Error diagnostics distinguish queue, byte-window and reader failures.
