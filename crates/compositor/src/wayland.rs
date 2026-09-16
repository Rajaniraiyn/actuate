//! Wayland client plumbing shared by input, capture and rendering: a
//! connection with its global list, logical output geometry, the seat's
//! keymap, and shared-memory buffers. Each consumer owns its own connection
//! and event queue; nothing here is a global singleton.
use actuate::{NativeError, Point, Result, geometry::Rect};
use memmap2::MmapMut;
use serde::Serialize;
use std::{fs::File, os::fd::AsFd};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    globals::{Global, GlobalList, GlobalListContents, registry_queue_init},
    protocol::{wl_buffer, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

pub fn fail(message: impl ToString) -> NativeError {
    NativeError::new("wayland", message)
}

/// One output with its logical layout position, as seen by clients.
#[derive(Debug, Clone, Serialize)]
pub struct Output {
    #[serde(skip)]
    pub wl: wl_output::WlOutput,
    pub name: String,
    pub description: String,
    /// Logical layout position and size in compositor coordinates.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Integer buffer scale advertised by wl_output; fractional scales round up.
    pub scale: i32,
    pub transform: i32,
    pub pixel_width: i32,
    pub pixel_height: i32,
    pub done: bool,
}
impl Output {
    /// Logical layout rectangle.
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.x as f64,
            y: self.y as f64,
            width: self.width as f64,
            height: self.height as f64,
        }
    }
    pub fn contains(&self, point: &Point) -> bool {
        self.rect().contains(point)
    }
}

/// Registry-driven output tracking. Embed it in a state type that
/// implements `AsMut<Outputs>` and delegate the output dispatches to it.
#[derive(Default)]
pub struct Outputs {
    pub outputs: Vec<Output>,
}
impl Outputs {
    /// Binds every advertised output and, when available, its xdg_output.
    pub fn bind<S>(&mut self, globals: &GlobalList, qh: &QueueHandle<S>)
    where
        S: Dispatch<wl_output::WlOutput, ()>
            + Dispatch<zxdg_output_v1::ZxdgOutputV1, wl_output::WlOutput>
            + Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()>
            + 'static,
    {
        let manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1> =
            globals.bind(qh, 3..=3, ()).ok();
        let mut outputs = Vec::new();
        globals.contents().with_list(|list: &[Global]| {
            for global in list.iter().filter(|g| g.interface == "wl_output") {
                let version = global.version.min(4);
                let wl: wl_output::WlOutput = globals.registry().bind(global.name, version, qh, ());
                if let Some(manager) = &manager {
                    manager.get_xdg_output(&wl, qh, wl.clone());
                }
                outputs.push(Output {
                    wl,
                    name: String::new(),
                    description: String::new(),
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    scale: 1,
                    transform: 0,
                    pixel_width: 0,
                    pixel_height: 0,
                    done: false,
                });
            }
        });
        self.outputs = outputs;
    }
    pub fn find(&self, point: &Point) -> Option<&Output> {
        self.outputs.iter().find(|o| o.done && o.contains(point))
    }
    pub fn by_name(&self, name: &str) -> Option<&Output> {
        self.outputs.iter().find(|o| o.name == name)
    }
    fn by_proxy(&mut self, output: &wl_output::WlOutput) -> Option<&mut Output> {
        self.outputs.iter_mut().find(|o| &o.wl == output)
    }
}
impl<S> Dispatch<wl_output::WlOutput, (), S> for Outputs
where
    S: Dispatch<wl_output::WlOutput, ()> + AsMut<Outputs>,
{
    fn event(
        state: &mut S,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
        let Some(record) = state.as_mut().by_proxy(output) else {
            return;
        };
        match event {
            wl_output::Event::Geometry { transform, .. } => {
                record.transform = transform.into_result().map(|t| t as i32).unwrap_or(0);
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                if flags
                    .into_result()
                    .is_ok_and(|f| f.contains(wl_output::Mode::Current))
                {
                    record.pixel_width = width;
                    record.pixel_height = height;
                }
            }
            wl_output::Event::Scale { factor } => record.scale = factor,
            wl_output::Event::Name { name } => record.name = name,
            wl_output::Event::Description { description } => record.description = description,
            wl_output::Event::Done => {
                record.done = true;
                if record.width == 0 && record.pixel_width > 0 {
                    // Without xdg_output, approximate the logical size from the mode.
                    let (w, h) = if record.transform % 2 == 1 {
                        (record.pixel_height, record.pixel_width)
                    } else {
                        (record.pixel_width, record.pixel_height)
                    };
                    record.width = w / record.scale.max(1);
                    record.height = h / record.scale.max(1);
                }
            }
            _ => {}
        }
    }
}
impl<S> Dispatch<zxdg_output_v1::ZxdgOutputV1, wl_output::WlOutput, S> for Outputs
where
    S: Dispatch<zxdg_output_v1::ZxdgOutputV1, wl_output::WlOutput> + AsMut<Outputs>,
{
    fn event(
        state: &mut S,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output: &wl_output::WlOutput,
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
        let Some(record) = state.as_mut().by_proxy(output) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                record.x = x;
                record.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                record.width = width;
                record.height = height;
            }
            zxdg_output_v1::Event::Name { name } if record.name.is_empty() => record.name = name,
            zxdg_output_v1::Event::Description { description } if record.description.is_empty() => {
                record.description = description
            }
            _ => {}
        }
    }
}
impl<S> Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, (), S> for Outputs
where
    S: Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()>,
{
    fn event(
        _: &mut S,
        _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
    }
}

/// The seat's keyboard keymap as advertised to ordinary clients. Virtual
/// keyboards that reuse it send key codes in the user's real layout.
#[derive(Default)]
pub struct SeatKeymap {
    pub seat: Option<wl_seat::WlSeat>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub keymap: Option<String>,
    pub name: String,
}
impl SeatKeymap {
    pub fn bind<S>(&mut self, globals: &GlobalList, qh: &QueueHandle<S>) -> Result<()>
    where
        S: Dispatch<wl_seat::WlSeat, ()> + Dispatch<wl_keyboard::WlKeyboard, ()> + 'static,
    {
        let seat: wl_seat::WlSeat = globals
            .bind(qh, 4..=9, ())
            .map_err(|e| fail(format!("wl_seat: {e}")))?;
        self.seat = Some(seat);
        Ok(())
    }
}
impl<S> Dispatch<wl_seat::WlSeat, (), S> for SeatKeymap
where
    S: Dispatch<wl_seat::WlSeat, ()>
        + Dispatch<wl_keyboard::WlKeyboard, ()>
        + AsMut<SeatKeymap>
        + 'static,
{
    fn event(
        state: &mut S,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<S>,
    ) {
        let record = state.as_mut();
        match event {
            wl_seat::Event::Capabilities { capabilities } => {
                let has_keyboard = capabilities
                    .into_result()
                    .is_ok_and(|c| c.contains(wl_seat::Capability::Keyboard));
                if has_keyboard && record.keyboard.is_none() {
                    record.keyboard = Some(seat.get_keyboard(qh, ()));
                }
            }
            wl_seat::Event::Name { name } => record.name = name,
            _ => {}
        }
    }
}
impl<S> Dispatch<wl_keyboard::WlKeyboard, (), S> for SeatKeymap
where
    S: Dispatch<wl_keyboard::WlKeyboard, ()> + AsMut<SeatKeymap>,
{
    fn event(
        state: &mut S,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
        if let wl_keyboard::Event::Keymap { format, fd, size } = event
            && format
                .into_result()
                .is_ok_and(|f| f == wl_keyboard::KeymapFormat::XkbV1)
        {
            let file = File::from(fd);
            // SAFETY: the compositor shares a read-only keymap file of `size` bytes.
            let map = unsafe { memmap2::MmapOptions::new().len(size as usize).map(&file) };
            if let Ok(map) = map {
                let text = String::from_utf8_lossy(&map);
                state.as_mut().keymap = Some(text.trim_end_matches('\0').to_owned());
            }
        }
    }
}

/// An ARGB8888 shared-memory buffer with CPU-writable pixels.
pub struct ShmBuffer {
    pub buffer: wl_buffer::WlBuffer,
    pub pool: wl_shm_pool::WlShmPool,
    pub map: MmapMut,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: wl_shm::Format,
    _file: File,
}
impl ShmBuffer {
    pub fn new<S>(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<S>,
        width: i32,
        height: i32,
        format: wl_shm::Format,
    ) -> Result<Self>
    where
        S: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
    {
        Self::with_stride(shm, qh, width, height, width * 4, format)
    }
    pub fn with_stride<S>(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<S>,
        width: i32,
        height: i32,
        stride: i32,
        format: wl_shm::Format,
    ) -> Result<Self>
    where
        S: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
    {
        if width <= 0 || height <= 0 || stride < width * 4 || stride > 1 << 20 || height > 1 << 15 {
            return Err(fail("Invalid buffer dimensions"));
        }
        let size = stride as usize * height as usize;
        let fd = rustix::fs::memfd_create("actuate-shm", rustix::fs::MemfdFlags::CLOEXEC)
            .map_err(fail)?;
        let file = File::from(fd);
        file.set_len(size as u64).map_err(fail)?;
        // SAFETY: the file is private, sized and stays alive with this struct.
        let map = unsafe { MmapMut::map_mut(&file) }.map_err(fail)?;
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, format, qh, ());
        Ok(Self {
            buffer,
            pool,
            map,
            width,
            height,
            stride,
            format,
            _file: file,
        })
    }
    /// Copies the buffer into packed RGBA rows, honoring the wl_shm layout.
    pub fn to_rgba(&self) -> Result<Vec<u8>> {
        let (w, h, stride) = (
            self.width as usize,
            self.height as usize,
            self.stride as usize,
        );
        let mut out = vec![0; w * h * 4];
        for y in 0..h {
            let row = &self.map[y * stride..y * stride + w * 4];
            for x in 0..w {
                let p = &row[x * 4..x * 4 + 4];
                let (r, g, b, a) = match self.format {
                    wl_shm::Format::Argb8888 => (p[2], p[1], p[0], p[3]),
                    wl_shm::Format::Xrgb8888 => (p[2], p[1], p[0], 255),
                    wl_shm::Format::Abgr8888 => (p[0], p[1], p[2], p[3]),
                    wl_shm::Format::Xbgr8888 => (p[0], p[1], p[2], 255),
                    other => return Err(fail(format!("Unsupported shm format {other:?}"))),
                };
                let o = (y * w + x) * 4;
                out[o..o + 4].copy_from_slice(&[r, g, b, a]);
            }
        }
        Ok(out)
    }
}
impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
    }
}

/// Connection plus the initial global list and its event queue.
pub struct Desktop<S: 'static> {
    pub connection: Connection,
    pub globals: GlobalList,
    pub queue: EventQueue<S>,
}
impl<S> Desktop<S>
where
    S: Dispatch<wl_registry::WlRegistry, GlobalListContents> + 'static,
{
    pub fn connect() -> Result<Self> {
        let connection =
            Connection::connect_to_env().map_err(|e| NativeError::new("wayland_unavailable", e))?;
        let (globals, queue) =
            registry_queue_init::<S>(&connection).map_err(|e| fail(format!("registry: {e}")))?;
        Ok(Self {
            connection,
            globals,
            queue,
        })
    }
    pub fn qh(&self) -> QueueHandle<S> {
        self.queue.handle()
    }
    pub fn roundtrip(&mut self, state: &mut S) -> Result<()> {
        self.queue
            .roundtrip(state)
            .map(|_| ())
            .map_err(|e| fail(format!("roundtrip: {e}")))
    }
}
impl<S> Desktop<S>
where
    S: Dispatch<wl_registry::WlRegistry, GlobalListContents>
        + Dispatch<wl_output::WlOutput, ()>
        + Dispatch<zxdg_output_v1::ZxdgOutputV1, wl_output::WlOutput>
        + Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()>
        + AsMut<Outputs>
        + 'static,
{
    /// Connects, binds every output and completes one round trip so output
    /// geometry is known before the caller binds its own globals.
    pub fn connect_with_outputs(state: &mut S) -> Result<Self> {
        let mut desktop = Self::connect()?;
        let qh = desktop.qh();
        state.as_mut().bind(&desktop.globals, &qh);
        desktop.roundtrip(state)?;
        Ok(desktop)
    }
}

/// A registry listener that ignores globals appearing after startup.
#[macro_export]
macro_rules! delegate_registry {
    ($state:ty) => {
        impl
            ::wayland_client::Dispatch<
                ::wayland_client::protocol::wl_registry::WlRegistry,
                ::wayland_client::globals::GlobalListContents,
            > for $state
        {
            fn event(
                _: &mut Self,
                _: &::wayland_client::protocol::wl_registry::WlRegistry,
                _: ::wayland_client::protocol::wl_registry::Event,
                _: &::wayland_client::globals::GlobalListContents,
                _: &::wayland_client::Connection,
                _: &::wayland_client::QueueHandle<Self>,
            ) {
            }
        }
    };
}
/// Delegates the shared dispatch families to their helper states.
#[macro_export]
macro_rules! delegate_outputs {
    ($state:ty) => {
        ::wayland_client::delegate_dispatch!($state: [::wayland_client::protocol::wl_output::WlOutput: ()] => $crate::wayland::Outputs);
        ::wayland_client::delegate_dispatch!($state: [::wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1: ::wayland_client::protocol::wl_output::WlOutput] => $crate::wayland::Outputs);
        ::wayland_client::delegate_dispatch!($state: [::wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1: ()] => $crate::wayland::Outputs);
    };
}
#[macro_export]
macro_rules! delegate_seat_keymap {
    ($state:ty) => {
        ::wayland_client::delegate_dispatch!($state: [::wayland_client::protocol::wl_seat::WlSeat: ()] => $crate::wayland::SeatKeymap);
        ::wayland_client::delegate_dispatch!($state: [::wayland_client::protocol::wl_keyboard::WlKeyboard: ()] => $crate::wayland::SeatKeymap);
    };
}
/// Ignores events from objects that never send any, or whose events are irrelevant.
#[macro_export]
macro_rules! delegate_silent {
    ($state:ty, [$($iface:ty),* $(,)?]) => {
        $(::wayland_client::delegate_noop!($state: ignore $iface);)*
    };
}
