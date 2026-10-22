#!/usr/bin/env python3
"""Minimal org.kde.StatusNotifierWatcher for headless tray tests.

Stands in for a desktop panel on a private session bus: it accepts item
registrations and reports a registered host, so StatusNotifierItem clients
(the Linux tray icon) come online. Each registered item's bus name is printed
on its own line, then flushed, so a test can address the item over D-Bus.
"""

import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

INTERFACE = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>
"""

items = []


def on_call(connection, sender, path, interface, method, parameters, invocation):
    if method == "RegisterStatusNotifierItem":
        (service,) = parameters.unpack()
        # Items register either a bus name or an object path on their own name.
        name = sender if service.startswith("/") else service
        items.append(name)
        print(name, flush=True)
        connection.emit_signal(
            None, path, interface, "StatusNotifierItemRegistered", GLib.Variant("(s)", (name,))
        )
    invocation.return_value(None)


def on_get(connection, sender, path, interface, name):
    return {
        "RegisteredStatusNotifierItems": GLib.Variant("as", items),
        "IsStatusNotifierHostRegistered": GLib.Variant("b", True),
        "ProtocolVersion": GLib.Variant("i", 0),
    }[name]


def on_bus(connection, name):
    info = Gio.DBusNodeInfo.new_for_xml(INTERFACE).interfaces[0]
    connection.register_object("/StatusNotifierWatcher", info, on_call, on_get, None)


def main():
    Gio.bus_own_name(
        Gio.BusType.SESSION,
        "org.kde.StatusNotifierWatcher",
        Gio.BusNameOwnerFlags.NONE,
        on_bus,
        None,
        lambda *_: sys.exit("could not own org.kde.StatusNotifierWatcher"),
    )
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
