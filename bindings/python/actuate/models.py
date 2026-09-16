from __future__ import annotations
from dataclasses import asdict, dataclass, field
from typing import Any, Literal, Mapping, Union

Effect = Literal["none", "dispatched", "unknown"]
Json = Union[None, bool, int, float, str, list["Json"], dict[str, "Json"]]

@dataclass(frozen=True, kw_only=True)
class ElementRef:
    session: str
    id: int

Target = Union[ElementRef, str]

@dataclass(frozen=True, kw_only=True)
class ObserveOptions:
    pid: int
    max_nodes: int = 1000
    max_depth: int = 30
    window_id: int | None = None

@dataclass(frozen=True, kw_only=True)
class Node:
    reference: ElementRef
    attributes: Mapping[str, Json]
    actions: tuple[str, ...]
    parameterized_attributes: tuple[str, ...]
    children: tuple[ElementRef, ...]
    issues: tuple[Json, ...]

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> Node:
        return cls(reference=ElementRef(**value["reference"]), attributes=value["attributes"],
                   actions=tuple(value["actions"]), parameterized_attributes=tuple(value["parameterized_attributes"]),
                   children=tuple(ElementRef(**v) for v in value["children"]), issues=tuple(value["issues"]))

@dataclass(frozen=True, kw_only=True)
class Snapshot:
    root: ElementRef
    nodes: tuple[Node, ...]
    complete: bool
    traversal_complete: bool
    revision: int
    issues: tuple[Json, ...]

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> Snapshot:
        return cls(root=ElementRef(**value["root"]), nodes=tuple(Node.from_wire(v) for v in value["nodes"]),
                   complete=value["complete"], traversal_complete=value["traversal_complete"],
                   revision=value["revision"], issues=tuple(value["issues"]))

@dataclass(frozen=True, kw_only=True)
class Receipt:
    effect: Effect
    route: str

@dataclass(frozen=True, kw_only=True)
class Perform:
    name: str
    kind: Literal["perform"] = field(default="perform", init=False)

@dataclass(frozen=True, kw_only=True)
class SetString:
    attribute: str
    value: str
    kind: Literal["set_string"] = field(default="set_string", init=False)

@dataclass(frozen=True, kw_only=True)
class Point:
    x: float
    y: float

@dataclass(frozen=True, kw_only=True)
class Delivery:
    kind: Literal["global", "process"]
    pid: int | None = None

    def to_wire(self) -> dict[str, Any]:
        if self.kind == "global":
            if self.pid is not None:
                raise ValueError("Global delivery does not accept a PID")
            return {"kind": "global"}
        if self.pid is None:
            raise ValueError("Process delivery requires a PID")
        return asdict(self)
