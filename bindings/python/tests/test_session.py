import asyncio
import sys
import threading
import unittest
from actuate import Actuate, AsyncActuate, NativeError, ObserveOptions, Operation

class FakeTransport:
    def __init__(self): self.calls = []; self.closed = 0
    def request(self, request): self.calls.append(request); return request.get("value")
    def close(self): self.closed += 1

class Sessions(unittest.TestCase):
    def test_middleware_extensions_and_lifetime(self):
        transport = FakeTransport()
        events = []
        def policy(request, next):
            events.append(request["op"])
            return next(request)
        with Actuate.from_transport(transport, middleware=(policy,)) as session:
            op = Operation(lambda value: dict(op="echo", value=value), str)
            self.assertEqual(session.run(op, 4), "4")
            with self.assertRaises(ValueError): session.request(dict(op="echo", value=float("nan")))
        self.assertEqual(events, ["echo"])
        session.close()
        self.assertEqual(transport.closed, 1)
        with self.assertRaises(NativeError) as error: session.capabilities()
        self.assertEqual(error.exception.code, "session_closed")
    def test_native_constructor_rejects_device_on_host(self):
        with self.assertRaises(NativeError) as error: Actuate.connect(device="invalid")
        self.assertEqual(error.exception.code, "invalid_request")
    @unittest.skipIf(sys.platform == "linux", "Needs a desktop accessibility bus")
    def test_native_provider(self):
        with Actuate.connect() as session:
            self.assertIsInstance(session.capabilities(), dict)
            with self.assertRaises(NativeError) as error: session.request(dict(op="does_not_exist"))
            self.assertEqual(error.exception.effect, "none")
            self.assertIsInstance(session.capabilities(), dict)

class AsyncSessions(unittest.IsolatedAsyncioTestCase):
    async def test_cancelled_caller_does_not_close_inflight_native_work(self):
        started, finish = threading.Event(), threading.Event()
        class Slow(FakeTransport):
            def request(self, request):
                started.set(); finish.wait(5)
                return super().request(request)
        transport = Slow()
        session = AsyncActuate.from_transport(transport)
        task = asyncio.create_task(session.request(dict(op="echo",value=2)))
        await asyncio.to_thread(started.wait, 5)
        task.cancel()
        with self.assertRaises(asyncio.CancelledError): await task
        closing = asyncio.create_task(session.close())
        await asyncio.sleep(0.02)
        self.assertEqual(transport.closed, 0)
        finish.set(); await closing
        self.assertEqual(len(transport.calls), 1)
        self.assertEqual(transport.closed, 1)
    @unittest.skipIf(sys.platform == "linux", "Needs a desktop accessibility bus")
    async def test_native_context(self):
        async with AsyncActuate.connect() as session:
            self.assertIsInstance(await session.capabilities(), dict)

if __name__ == "__main__": unittest.main()
