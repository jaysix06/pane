-- Keyboard shortcuts: Pane's commands, their default keys, and the person's
-- own keys from Settings (settings.shortcuts: command id to a key, or "" for
-- none). app.luau runs them; Settings → Keyboard shortcuts rebinds them.
--
-- A key is written lower case, modifiers first in this order:
-- "ctrl+shift+alt+<key>", the key as named below ("left", "f5", "delete",
-- ",", "a").

local state = require("./state")
local commands = require("./components/commands")

export type Command = { id: string, label: string, keys: { string }, run: () -> () }

local M = {}

local function toggle_setting(key: string)
	ui.invoke("set_setting", { key = key, value = not state.setting(key) })
end

local function focus_search()
	local el = state.search_ref:peek()
	if el then
		el:focus()
	end
end

local function edit_address()
	local n = state.nav:peek()
	state.address_text:set(if n then n.address else "")
	state.editing_address:set(true)
end

-- Set by settings.luau (it requires this module): opens the Settings window.
M.open_settings = function() end

M.list = {
	{ id = "back", label = "Back", keys = { "alt+left" }, run = function() ui.invoke("back") end },
	{ id = "forward", label = "Forward", keys = { "alt+right" }, run = function() ui.invoke("forward") end },
	{ id = "up", label = "Up one folder", keys = { "alt+up", "backspace" }, run = function() ui.invoke("up") end },
	{ id = "refresh", label = "Refresh", keys = { "f5", "ctrl+r" }, run = function() ui.invoke("refresh") end },
	{ id = "address", label = "Edit the address", keys = { "ctrl+l", "alt+d", "f4" }, run = edit_address },
	{ id = "search", label = "Search this folder", keys = { "ctrl+f", "f3", "ctrl+e" }, run = focus_search },
	{ id = "new_tab", label = "New tab", keys = { "ctrl+t" }, run = function() ui.invoke("new_tab") end },
	{ id = "close_tab", label = "Close tab", keys = { "ctrl+w" }, run = function() ui.invoke("close_tab") end },
	{ id = "next_tab", label = "Next tab", keys = { "ctrl+tab" }, run = function() ui.invoke("next_tab") end },
	{ id = "prev_tab", label = "Previous tab", keys = { "ctrl+shift+tab" }, run = function() ui.invoke("prev_tab") end },
	{ id = "select_all", label = "Select all", keys = { "ctrl+a" }, run = function() ui.invoke("select_all") end },
	{ id = "copy", label = "Copy", keys = { "ctrl+c" }, run = commands.copy },
	{ id = "cut", label = "Cut", keys = { "ctrl+x" }, run = commands.cut },
	{ id = "copy_path", label = "Copy path", keys = { "ctrl+shift+c" }, run = commands.copy_path },
	{ id = "paste", label = "Paste", keys = { "ctrl+v" }, run = commands.paste },
	{ id = "new_folder", label = "New folder", keys = { "ctrl+shift+n" }, run = commands.new_folder },
	{ id = "rename", label = "Rename", keys = { "f2" }, run = commands.rename },
	{ id = "delete", label = "Delete", keys = { "delete" }, run = function() commands.delete(false) end },
	{
		id = "delete_permanently",
		label = "Delete permanently",
		keys = { "shift+delete" },
		run = function() commands.delete(true) end,
	},
	{ id = "preview", label = "Show or hide the preview pane", keys = { "alt+p" }, run = function() toggle_setting("preview_pane") end },
	{ id = "settings", label = "Settings", keys = { "ctrl+," }, run = function() M.open_settings() end },
} :: { Command }

-- Keys that stay as they are: Enter opens, Escape clears the selection,
-- Ctrl+1 to Ctrl+9 pick a tab.
M.fixed = { "enter", "escape", "ctrl+1 … ctrl+9" }

local NAMES = {
	arrowleft = "left",
	arrowright = "right",
	arrowup = "up",
	arrowdown = "down",
	[" "] = "space",
	spacebar = "space",
	esc = "escape",
	del = "delete",
	return_ = "enter",
}

-- A key event as a key: "ctrl+shift+n". Nil for a modifier alone.
function M.key_of(e: any): string?
	local key = string.lower(e.key or "")
	if key == "" or key == "control" or key == "shift" or key == "alt" or key == "meta" then
		return nil
	end
	key = NAMES[key] or key
	local parts = {}
	if e.ctrl or e.meta then
		table.insert(parts, "ctrl")
	end
	if e.shift then
		table.insert(parts, "shift")
	end
	if e.alt then
		table.insert(parts, "alt")
	end
	table.insert(parts, key)
	return table.concat(parts, "+")
end

-- "ctrl+shift+n" as people read it: "Ctrl+Shift+N".
function M.label(key: string): string
	if key == "" then
		return "None"
	end
	local out = {}
	for part in string.gmatch(key, "[^+]+") do
		local named = ({
			ctrl = "Ctrl",
			shift = "Shift",
			alt = "Alt",
			left = "Left",
			right = "Right",
			up = "Up",
			down = "Down",
			backspace = "Backspace",
			delete = "Del",
			tab = "Tab",
			enter = "Enter",
			escape = "Esc",
			space = "Space",
		})[part]
		table.insert(out, named or string.upper(part))
	end
	-- "ctrl++" (a plus key) splits to nothing after ctrl.
	if string.sub(key, -1) == "+" and string.sub(key, -2) == "++" then
		table.insert(out, "+")
	end
	return table.concat(out, "+")
end

-- A command's keys now: the person's (one, or none) or its defaults.
function M.keys(command: Command): { string }
	local own = (state.setting("shortcuts") or {})[command.id]
	if own == nil then
		return command.keys
	end
	return if own == "" then {} else { own }
end

function M.is_custom(command: Command): boolean
	return (state.setting("shortcuts") or {})[command.id] ~= nil
end

-- A command's name with its first key, for tooltips: "Copy (Ctrl+C)".
function M.hint(id: string): string
	for _, c in M.list do
		if c.id == id then
			local keys = M.keys(c)
			return if #keys > 0 then `{c.label} ({M.label(keys[1])})` else c.label
		end
	end
	return id
end

-- Just the key that runs command `id`, as people read it ("Ctrl+C"), or
-- "" when it has none: for menus, which show the label beside it.
function M.key(id: string): string
	for _, c in M.list do
		if c.id == id then
			local keys = M.keys(c)
			return if #keys > 0 then M.label(keys[1]) else ""
		end
	end
	return ""
end

-- The command `key` runs, if any.
function M.command_for(key: string): Command?
	for _, c in M.list do
		for _, k in M.keys(c) do
			if k == key then
				return c
			end
		end
	end
	return nil
end

-- Gives `id` the key `key` ("" for none, nil for its defaults), taking it
-- from any other command that had it.
function M.set(id: string, key: string?)
	local own = table.clone(state.setting("shortcuts") or {})
	if key and key ~= "" then
		for _, c in M.list do
			if c.id ~= id and table.find(M.keys(c), key) then
				-- The other command keeps its remaining defaults, if any.
				local rest = {}
				for _, k in M.keys(c) do
					if k ~= key then
						table.insert(rest, k)
					end
				end
				own[c.id] = rest[1] or ""
			end
		end
	end
	own[id] = key
	ui.invoke("set_setting", { key = "shortcuts", value = own })
end

return M
