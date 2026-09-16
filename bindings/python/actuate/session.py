from __future__ import annotations
import asyncio
import json
import threading
from dataclasses import asdict, is_dataclass
from typing import Any, Callable, Generic, Mapping, Protocol, TypeVar
from .models import Delivery, Effect, Json, Node, ObserveOptions, Receipt, Snapshot, Target

class NativeError(RuntimeError):
    def __init__(self, code: str, message: str, effect: Effect = "none") -> None:
        super().__init__(message)
        self.code, self.message, self.effect = code, message, effect

class Transport(Protocol):
    def request(self, request: dict[str, Json]) -> Json: ...
    def close(self) -> None: ...

Input = TypeVar("Input")
Output = TypeVar("Output")
class Operation(Generic[Input, Output]):
    def __init__(self, request: Callable[[Input], dict[str, Json]], parse: Callable[[Json], Output]) -> None:
        self.request, self.parse = request, parse

Next = Callable[[dict[str, Json]], Json]
Middleware = Callable[[dict[str, Json], Next], Json]

def _wire(value: Any) -> Any:
    if isinstance(value, Delivery):
        return value.to_wire()
    if is_dataclass(value) and not isinstance(value, type):
        return asdict(value)
    return value

def _unwrap(text: str) -> Any:
    envelope = json.loads(text)
    if "error" in envelope:
        raise NativeError(**envelope["error"])
    return envelope["result"]

class _NativeTransport:
    def __init__(self, options: dict[str, Any]) -> None:
        try:
            from ._native import NativeSession
        except ImportError as error:
            raise NativeError("binding_not_installed", "Install a matching release wheel or build the binding with maturin") from error
        try:
            self._native = NativeSession(json.dumps(options, allow_nan=False))
        except RuntimeError as error:
            try:
                detail = json.loads(str(error))
            except ValueError:
                raise error
            raise NativeError(**detail) from error

    def request(self, request: dict[str, Json]) -> Json:
        return _unwrap(self._native.request(json.dumps(request, allow_nan=False)))

    def close(self) -> None:
        _unwrap(self._native.close())

class Session:
    def __init__(self, transport: Transport, *, middleware: tuple[Middleware, ...] = ()) -> None:
        self._transport = transport
        self._closed = False
        self._lock = threading.Lock()
        execute = transport.request
        for wrap in reversed(middleware):
            def wrapped(request: dict[str, Json], wrap: Middleware = wrap, next: Next = execute) -> Json:
                return wrap(request, next)
            execute = wrapped
        self._execute = execute

    def request(self, request: Mapping[str, Any]) -> Any:
        # Copy before dispatch so callers cannot change an in-flight request.
        owned = json.loads(json.dumps(request, default=_wire, allow_nan=False))
        with self._lock:
            if self._closed:
                raise NativeError("session_closed", "The session is closed")
            return self._execute(owned)

    def run(self, operation: Operation[Input, Output], value: Input) -> Output:
        return operation.parse(self.request(operation.request(value)))

    def capabilities(self) -> Json: return self.request({"op": "capabilities"})
    def discover(self) -> Json: return self.request({"op": "discover"})
    def windows(self) -> Json: return self.request({"op": "windows"})
    def displays(self) -> Json: return self.request({"op": "displays"})

    def observe(self, *, pid: int | None = None, max_nodes: int = 1000, max_depth: int = 30,
                window_id: int | None = None, options: ObserveOptions | None = None) -> Snapshot:
        if options is not None:
            if pid is not None or max_nodes != 1000 or max_depth != 30 or window_id is not None:
                raise ValueError("Pass options or individual observation arguments, not both")
        else:
            if pid is None: raise ValueError("pid is required")
            options = ObserveOptions(pid=pid, max_nodes=max_nodes, max_depth=max_depth, window_id=window_id)
        request: dict[str, Any] = {"op": "observe", "request": {"pid": options.pid, "max_nodes": options.max_nodes, "max_depth": options.max_depth}}
        if options.window_id is not None: request["window_id"] = options.window_id
        return Snapshot.from_wire(self.request(request))

    def inspect(self, target: Target) -> Node:
        return Node.from_wire(self.request({"op": "inspect", "target": _wire(target)}))
    def semantic(self, target: Target, action: Any) -> Receipt:
        return Receipt(**self.request({"op": "semantic", "target": _wire(target), "action": _wire(action)}))
    def pointer(self, delivery: Delivery, action: Mapping[str, Any]) -> Receipt:
        return Receipt(**self.request({"op": "pointer", "delivery": _wire(delivery), "action": action}))
    def text(self, delivery: Delivery, text: str) -> Receipt:
        return Receipt(**self.request({"op": "text", "delivery": _wire(delivery), "text": text}))
    def key(self, delivery: Delivery, *, key_code: int, modifiers: Mapping[str, bool] | None = None) -> Receipt:
        return Receipt(**self.request({"op": "key", "delivery": _wire(delivery), "chord": {"key_code": key_code, "modifiers": dict(modifiers or {})}}))
    def capture(self, path: str, **options: Any) -> Json:
        return self.request({**options, "op": "capture", "path": path})
    def cursor(self, command: Mapping[str, Any]) -> Json: return self.request({"op": "cursor", "command": command})
    def cursor_overlay(self, action: Mapping[str, Any]) -> Json: return self.request({"op": "cursor_overlay", "action": action})
    def cursor_state(self) -> Json: return self.request({"op": "cursor_state"})
    def snapshots(self) -> Json: return self.request({"op": "snapshots"})
    def diff(self, before: int, *, after: int | None = None) -> Json:
        return self.request({"op": "diff", "before": before, "after": after})
    def close(self) -> None:
        with self._lock:
            if not self._closed:
                self._closed = True
                self._transport.close()
    def __enter__(self) -> Session: return self
    def __exit__(self, *_: Any) -> None: self.close()

class Actuate:
    @staticmethod
    def connect(*, provider: str = "native", device: str | None = None, device_set: str | None = None,
                credentials: str | None = None, trust_first_connection: bool = False) -> Session:
        return Session(_NativeTransport(dict(provider=provider, device=device, device_set=device_set,
                                            credentials=credentials, trust_first_connection=trust_first_connection)))
    @staticmethod
    def from_transport(transport: Transport, *, middleware: tuple[Middleware, ...] = ()) -> Session:
        return Session(transport, middleware=middleware)

class AsyncSession:
    def __init__(self, session: Session) -> None:
        self._session = session
        self._pending: set[asyncio.Task[Any]] = set()
        self._closing: asyncio.Task[None] | None = None

    async def _call(self, method: Callable[..., Output], *args: Any, **kwargs: Any) -> Output:
        if self._closing is not None: raise NativeError("session_closed", "The session is closed")
        task = asyncio.create_task(asyncio.to_thread(method, *args, **kwargs))
        self._pending.add(task)
        def finished(done: asyncio.Task[Any]) -> None:
            self._pending.discard(done)
            if not done.cancelled(): done.exception()  # Consume failures after caller cancellation.
        task.add_done_callback(finished)
        return await asyncio.shield(task)

    async def request(self, request: Mapping[str, Any]) -> Any: return await self._call(self._session.request, request)
    async def run(self, operation: Operation[Input, Output], value: Input) -> Output: return await self._call(self._session.run, operation, value)
    async def capabilities(self) -> Json: return await self._call(self._session.capabilities)
    async def discover(self) -> Json: return await self._call(self._session.discover)
    async def windows(self) -> Json: return await self._call(self._session.windows)
    async def displays(self) -> Json: return await self._call(self._session.displays)
    async def observe(self, *, pid: int | None = None, max_nodes: int = 1000, max_depth: int = 30,
                      window_id: int | None = None, options: ObserveOptions | None = None) -> Snapshot:
        return await self._call(self._session.observe, pid=pid, max_nodes=max_nodes, max_depth=max_depth, window_id=window_id, options=options)
    async def inspect(self, target: Target) -> Node: return await self._call(self._session.inspect, target)
    async def semantic(self, target: Target, action: Any) -> Receipt: return await self._call(self._session.semantic, target, action)
    async def pointer(self, delivery: Delivery, action: Mapping[str, Any]) -> Receipt: return await self._call(self._session.pointer, delivery, action)
    async def text(self, delivery: Delivery, text: str) -> Receipt: return await self._call(self._session.text, delivery, text)
    async def key(self, delivery: Delivery, *, key_code: int, modifiers: Mapping[str, bool] | None = None) -> Receipt:
        return await self._call(self._session.key, delivery, key_code=key_code, modifiers=modifiers)
    async def capture(self, path: str, **options: Any) -> Json: return await self._call(self._session.capture, path, **options)
    async def cursor(self, command: Mapping[str, Any]) -> Json: return await self._call(self._session.cursor, command)
    async def cursor_overlay(self, action: Mapping[str, Any]) -> Json: return await self._call(self._session.cursor_overlay, action)
    async def cursor_state(self) -> Json: return await self._call(self._session.cursor_state)
    async def snapshots(self) -> Json: return await self._call(self._session.snapshots)
    async def diff(self, before: int, *, after: int | None = None) -> Json: return await self._call(self._session.diff, before, after=after)
    async def close(self) -> None:
        async def finish() -> None:
            if self._pending: await asyncio.gather(*tuple(self._pending), return_exceptions=True)
            await asyncio.to_thread(self._session.close)
        if self._closing is None: self._closing = asyncio.create_task(finish())
        await asyncio.shield(self._closing)
    async def __aenter__(self) -> AsyncSession: return self
    async def __aexit__(self, *_: Any) -> None: await self.close()

class _Connection:
    def __init__(self, options: dict[str, Any]) -> None:
        self.options = options
        self.session: AsyncSession | None = None
    async def __aenter__(self) -> AsyncSession:
        task = asyncio.create_task(asyncio.to_thread(Actuate.connect, **self.options))
        try:
            self.session = AsyncSession(await asyncio.shield(task))
        except asyncio.CancelledError:
            def cleanup(done: asyncio.Task[Session]) -> None:
                if not done.cancelled() and done.exception() is None:
                    asyncio.create_task(asyncio.to_thread(done.result().close))
            task.add_done_callback(cleanup)
            raise
        return self.session
    async def __aexit__(self, *_: Any) -> None:
        if self.session is not None: await self.session.close()

class AsyncActuate:
    @staticmethod
    def connect(*, provider: str = "native", device: str | None = None, device_set: str | None = None,
                credentials: str | None = None, trust_first_connection: bool = False) -> _Connection:
        return _Connection(dict(provider=provider, device=device, device_set=device_set,
                                credentials=credentials, trust_first_connection=trust_first_connection))
    @staticmethod
    def from_transport(transport: Transport, *, middleware: tuple[Middleware, ...] = ()) -> AsyncSession:
        return AsyncSession(Session(transport, middleware=middleware))
