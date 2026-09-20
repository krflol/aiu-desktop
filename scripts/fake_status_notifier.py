#!/usr/bin/env python3
"""Controlled StatusNotifierWatcher and DBusMenu host for Linux smoke tests."""
import sys
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

XML = """<node>
<interface name='org.kde.StatusNotifierWatcher'>
 <method name='RegisterStatusNotifierItem'><arg type='s' direction='in' name='service'/></method>
</interface>
<interface name='org.freedesktop.DBus.Properties'>
 <method name='Get'><arg type='s' direction='in'/><arg type='s' direction='in'/><arg type='v' direction='out'/></method>
 <method name='GetAll'><arg type='s' direction='in'/><arg type='a{sv}' direction='out'/></method>
</interface>
<interface name='org.aiu.Smoke'>
 <method name='Invoke'><arg type='s' direction='in' name='label'/></method>
</interface></node>"""

connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
loop = GLib.MainLoop()
registered = {"service": None, "path": "/StatusNotifierItem"}

def menu_proxy():
    service, path = registered["service"], registered["path"]
    item = Gio.DBusProxy.new_sync(connection, Gio.DBusProxyFlags.NONE, None, service, path, "org.freedesktop.DBus.Properties", None)
    value = item.call_sync("Get", GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", "Menu")), Gio.DBusCallFlags.NONE, 3000, None).unpack()[0]
    return Gio.DBusProxy.new_sync(connection, Gio.DBusProxyFlags.NONE, None, service, value, "com.canonical.dbusmenu", None)

def find_id(node, wanted):
    ident, props, children = node
    if props.get("label") == wanted: return int(ident)
    for child in children:
        found = find_id(child, wanted)
        if found is not None: return found
    return None

def invoke(label):
    menu = menu_proxy()
    tree = menu.call_sync("GetLayout", GLib.Variant("(iias)", (0, -1, [])), Gio.DBusCallFlags.NONE, 3000, None).unpack()[1]
    ident = find_id(tree, {"show": "Show AIU", "refresh": "Refresh usage", "quit": "Quit AIU"}[label])
    if ident is None: raise RuntimeError(f"menu item not found: {label}")
    menu.call_sync("Event", GLib.Variant("(isvu)", (ident, "clicked", GLib.Variant("s", ""), 0)), Gio.DBusCallFlags.NONE, 3000, None)
    print(f"invoked-{label}", flush=True)

def method_call(conn, sender, path, interface, method, params, invocation):
    if interface == "org.kde.StatusNotifierWatcher" and method == "RegisterStatusNotifierItem":
        value = params.unpack()[0]
        if value.startswith("/"): registered.update(service=sender, path=value)
        else:
            parts = value.split("/", 1); registered.update(service=parts[0], path="/" + parts[1] if len(parts) == 2 else "/StatusNotifierItem")
        invocation.return_value(GLib.Variant("()", ())); print("registered", flush=True)
    elif interface == "org.freedesktop.DBus.Properties" and method == "Get":
        invocation.return_value(GLib.Variant("(v)", (GLib.Variant("b", True),)))
    elif interface == "org.freedesktop.DBus.Properties" and method == "GetAll":
        invocation.return_value(GLib.Variant("(a{sv})", ({"IsStatusNotifierHostRegistered": GLib.Variant("b", True)},)))
    elif interface == "org.aiu.Smoke" and method == "Invoke":
        try: invoke(params.unpack()[0]); invocation.return_value(GLib.Variant("()", ()))
        except Exception as error: invocation.return_dbus_error("org.aiu.Smoke.Error", str(error))

node = Gio.DBusNodeInfo.new_for_xml(XML)
for interface in node.interfaces:
    path = "/org/aiu/Smoke" if interface.name == "org.aiu.Smoke" else "/StatusNotifierWatcher"
    connection.register_object(path, interface, method_call, None, None)
Gio.bus_own_name_on_connection(connection, "org.kde.StatusNotifierWatcher", Gio.BusNameOwnerFlags.NONE, None, None)
Gio.bus_own_name_on_connection(connection, "org.aiu.Smoke", Gio.BusNameOwnerFlags.NONE, None, None)
loop.run()
