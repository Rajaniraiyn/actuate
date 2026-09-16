"""Fixture transport adapter exercising the compiled Python binding."""
import json
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bindings/python"))
from actuate import Actuate, NativeError

with Actuate.connect() as session:
    for line in sys.stdin:
        request = json.loads(line)
        identifier = request.pop("id", None)
        try:
            result = dict(result=session.request(request))
        except NativeError as error:
            result = dict(error=dict(code=error.code,message=error.message,effect=error.effect))
        print(json.dumps(dict(id=identifier, **result)), flush=True)
