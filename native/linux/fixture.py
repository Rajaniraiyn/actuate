#!/usr/bin/env python3
"""Disposable GTK4 fixture for Linux validation. Not linked or required by the library.

Runs on Wayland by default; set GDK_BACKEND=x11 to run under Xwayland.
Events append to the file named by ACTUATE_FIXTURE_LOG, one line each.
"""
import os
import sys
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk, GLib  # noqa: E402

LOG = os.environ.get("ACTUATE_FIXTURE_LOG")


def log(line):
    if LOG:
        with open(LOG, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    print(line, flush=True)


class Fixture(Gtk.Application):
    def __init__(self):
        super().__init__(application_id=os.environ.get("ACTUATE_FIXTURE_ID", "dev.actuate.fixture"))

    def do_activate(self):
        win = Gtk.ApplicationWindow(application=self, title=os.environ.get("ACTUATE_FIXTURE_TITLE", "Actuate Fixture"))
        win.set_default_size(520, 640)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=12, margin_bottom=12, margin_start=12, margin_end=12)
        win.set_child(box)
        self.status = Gtk.Label(label="status: idle", xalign=0)
        self.status.update_property([Gtk.AccessibleProperty.LABEL], ["status: idle"])
        box.append(self.status)
        self.clicks = 0

        def clicked(_button):
            self.clicks += 1
            self.set_status(f"clicked:{self.clicks}")

        button = Gtk.Button(label="Increment")
        button.connect("clicked", clicked)
        box.append(button)

        entry = Gtk.Entry(placeholder_text="Type here")
        entry.connect("changed", lambda e: self.set_status(f"entry:{e.get_text()}"))
        entry.connect("activate", lambda e: self.set_status(f"activated:{e.get_text()}"))
        box.append(entry)

        check = Gtk.CheckButton(label="Enable feature")
        check.connect("toggled", lambda c: self.set_status(f"checked:{c.get_active()}"))
        box.append(check)

        slider = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        slider.set_value(25)
        slider.get_adjustment().connect("value-changed", lambda a: self.set_status(f"slider:{a.get_value():.0f}"))
        box.append(slider)

        scrolled = Gtk.ScrolledWindow(vexpand=True)
        rows = Gtk.ListBox()
        rows.set_selection_mode(Gtk.SelectionMode.SINGLE)
        for i in range(30):
            rows.append(Gtk.Label(label=f"Row {i}", xalign=0, margin_top=6, margin_bottom=6))
        rows.connect("row-activated", lambda _l, r: self.set_status(f"row:{r.get_index()}"))
        rows.connect("row-selected", lambda _l, r: r and self.set_status(f"selected:{r.get_index()}"))
        scrolled.set_child(rows)
        box.append(scrolled)

        text = Gtk.TextView(accepts_tab=False)
        text.get_buffer().set_text("editor")
        text.get_buffer().connect("changed", lambda b: self.set_status(f"text:{b.get_text(b.get_start_iter(), b.get_end_iter(), True)}"))
        frame = Gtk.Frame(child=text)
        frame.set_size_request(-1, 80)
        box.append(frame)

        def focus_changed(window, _param):
            widget = window.get_focus()
            label = None
            if widget is not None:
                label = getattr(widget, "get_label", lambda: None)() or widget.get_name() or type(widget).__name__
            log(f"focus:{label}")

        win.connect("notify::focus-widget", focus_changed)
        controller = Gtk.EventControllerKey()
        controller.connect("key-pressed", lambda _c, keyval, keycode, state: log(f"key:{Gtk.accelerator_name(keyval, state)}") or False)
        win.add_controller(controller)
        win.present()
        log("ready")

    def set_status(self, value):
        self.status.set_label(f"status: {value}")
        log(value)


if __name__ == "__main__":
    sys.exit(Fixture().run(None))
