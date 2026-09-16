export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type NativeRequest = { op: string; [key: string]: Json | undefined };
export type Effect = "none" | "dispatched" | "unknown";
export type Provider = "native" | "windows" | "macos" | "linux" | "apple-simulator" | "apple-device" | "android";
export interface ConnectOptions {
  provider?: Provider;
  device?: string;
  deviceSet?: string;
  credentials?: string;
  trustFirstConnection?: boolean;
}
export interface ElementRef { readonly session: string; readonly id: number }
export type Target = ElementRef | `@e${number}`;
export interface Node {
  readonly reference: ElementRef;
  readonly attributes: Readonly<Record<string, Json>>;
  readonly actions: readonly string[];
  readonly parameterized_attributes: readonly string[];
  readonly children: readonly ElementRef[];
  readonly issues: readonly Json[];
}
export interface Snapshot {
  readonly root: ElementRef;
  readonly nodes: readonly Node[];
  readonly complete: boolean;
  readonly traversal_complete: boolean;
  readonly revision: number;
  readonly issues: readonly Json[];
}
export interface Receipt { readonly effect: Effect; readonly route: string }
export interface ObserveOptions { pid: number; maxNodes?: number; maxDepth?: number; windowId?: number }
export const observeOptions = <const T extends ObserveOptions>(options: T): Readonly<T> => Object.freeze({ ...options });
export interface Point { x: number; y: number }
export type Delivery = { kind: "global" } | { kind: "process"; pid: number };
export interface Modifiers { shift?: boolean; control?: boolean; alt?: boolean; meta?: boolean }
export type SemanticAction =
  | { kind: "perform"; name: string }
  | { kind: "set_string"; attribute: string; value: string }
  | { kind: "set_bool"; attribute: string; value: boolean }
  | { kind: "set_integer" | "set_float"; attribute: string; value: number }
  | { kind: "set_range"; attribute: string; location: number; length: number }
  | { kind: "set_point"; attribute: string; value: Point }
  | { kind: "set_size"; attribute: string; width: number; height: number };
export type MouseButton = "left" | "right" | "middle";
export type PointerAction =
  | { kind: "move"; point: Point }
  | { kind: "click"; point: Point; button?: MouseButton; count?: number; modifiers?: Modifiers }
  | { kind: "scroll"; vertical: number; horizontal: number; point?: Point }
  | { kind: "drag"; from: Point; to: Point; button?: MouseButton; modifiers?: Modifiers; duration_ms?: number };
export interface KeyChord { key_code: number; modifiers?: Modifiers }
export type CursorScope = { kind: "desktop" } | { kind: "window"; window_id: number; pid: number };
export type CursorCommand =
  | { op: "move"; x: number; y: number; duration_ms?: number }
  | { op: "click"; x: number; y: number }
  | { op: "scope"; scope: CursorScope }
  | { op: "configure"; appearance: Record<string, Json> }
  | { op: "hide" | "show" | "quit" };
export type CursorOverlayAction = { kind: "stop" } | {
  kind: "start"; executable?: string;
  physical_cursor?: "preserve" | "hide_within_scope" | "hide_while_visible";
  tracking?: "commands" | "physical_pointer";
};
export interface Transport { request(request: NativeRequest): Promise<Json>; close(): Promise<void> }
export type Next = (request: NativeRequest) => Promise<Json>;
export type Middleware = (request: NativeRequest, next: Next) => Promise<Json>;
export interface Operation<Input, Output> {
  request(input: Input): NativeRequest;
  parse(value: Json): Output;
}
export const defineOperation = <Input, Output>(operation: Operation<Input, Output>) => operation;
