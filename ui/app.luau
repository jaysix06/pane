-- Pane's window: tabs in the title bar, a toolbar, the sidebar beside the
-- folder view, and a status bar. Rust owns the model (state.luau says what
-- it publishes); this file lays it out and maps keys to actions.

local state = require("./state")
local theme = require("./theme")
local Tabs = require("./components/tabs")
local Toolbar = require("./components/toolbar")
local Sidebar = require("./components/sidebar")
local Views = require("./components/views")
local Overview = require("./components/overview")
local Preview = require("./components/preview")
local customize = require("./components/customize")
local confirm = require("./components/confirm")
local shortcuts = require("./shortcuts")
local settings_window = require("./settings")
local OpsPanel = require("./components/ops_panel")
local update_modal = require("./components/update_modal")
local about = require("./components/about")
local meter = require("./components/meter")

-- Pane Amber, dark or light as Settings say (theme.luau).
theme.follow(state.setting)

-- Messages from Rust (an error, a recovered settings file) as toasts.
local last_toast = 0
ui.effect(function()
	local t = state.toast:get()
	if t and t.at ~= last_toast then
		last_toast = t.at
		ui.toast { title = t.message, duration = 5 }
	end
end)

-- Window-wide shortcuts, as Explorer has them.
local function on_key(e): boolean
	if e.editing then
		return false
	end
	-- A rename field has the keyboard; if its row is gone, let go.
	if state.renaming:peek() ~= nil then
		local id = state.renaming:peek()
		if state.rows:index_of("id", id) ~= nil then
			return false
		end
		state.renaming:set(nil)
	end
	local key = e.key
	local ctrl = e.ctrl or e.meta
	-- The person's keys (Settings → Keyboard shortcuts), then the fixed ones.
	local pressed = shortcuts.key_of(e)
	local command = if pressed then shortcuts.command_for(pressed) else nil
	if command then
		command.run()
	elseif key == "Enter" then
		if ui.now() - state.rename_ended_at < 400 then
			return true
		end
		if state.sel:peek().count > 0 then
			ui.invoke("open_selection")
		end
	elseif key == "Escape" then
		ui.invoke("select_none")
	elseif ctrl and #key == 1 and key >= "1" and key <= "9" then
		local n = tonumber(key) :: number
		ui.invoke("switch_tab", if n == 9 then #state.tabs:peek() else n)
	else
		return false
	end
	return true
end

-- The footer: three cells with hairline dividers. The selection ("14 items
-- · 3 selected · 13 KB"), running file operations (ops_panel.luau), and the
-- drive's free space with a mini meter (a click opens the Overview). The
-- operations cell gives way first when the window narrows.

local function divider(show: (() -> boolean)?): Node
	local line = ui.div { w = 1, h = 14, shrink = 0, bg = "$border.subtle" }
	return if show then ui.show(show, line) else line
end

-- The drive the folder on screen is on, from the sidebar's places.
local current_drive = ui.computed(function(): state.Place?
	local n = state.nav:get()
	local p = state.places:get()
	if n == nil or p == nil or n.this_pc or n.overview or n.path == nil then
		return nil
	end
	local letter = string.lower(string.sub(n.path, 1, 2))
	if string.sub(letter, 2, 2) ~= ":" then
		return nil
	end
	for _, d in p.drives do
		if string.lower(string.sub(d.path, 1, 2)) == letter then
			return d
		end
	end
	return nil
end)

local function selection_cell(): Node
	local function text(): string
		local s = state.status:get() :: any
		local parts = {}
		for _, k in { "items", "selected", "size" } do
			if s[k] and s[k] ~= "" then
				table.insert(parts, s[k])
			end
		end
		return table.concat(parts, " · ")
	end
	return ui.text {
		text,
		color = "$text.tertiary",
		font = "$font.mono",
		text_size = 11.5,
		truncate = true,
		min_w = 0,
		shrink = 1,
	}
end

local function space_cell(): Node
	local function d(): any
		return current_drive:get()
	end
	return ui.button {
		label = "Storage",
		tooltip = ui.text {
			function()
				local x = d()
				return if x then `{x.label}: {x.used_of or ""}. Click for the Storage Overview.` else ""
			end,
			literal = true,
		},
		on_click = function()
			local x = d()
			if x then
				ui.invoke("navigate", "::overview")
				ui.invoke("storage_select", string.upper(string.sub(x.path, 1, 2)))
			end
		end,
		h = 22,
		px = 8,
		gap = 8,
		items = "center",
		shrink = 0,
		rounded = "$radius.sm",
		bg = "#00000000",
		color = "$text.tertiary",
		hover = { bg = "$hover", color = "$text.secondary" },
		ui.text {
			function()
				local x = d()
				return if x then string.upper(string.sub(x.path, 1, 2)) else ""
			end,
			font = "$font.mono",
			text_size = 11.5,
			color = "$text.secondary",
		},
		meter.meter {
			data = function()
				local x = d()
				return meter.drive_data(state.storage:get(), if x then string.sub(x.path, 1, 2) else "", if x then x.used or 0 else 0)
			end,
			glow = function()
				return (state.setting("glow") or 70) / 100 * 0.6
			end,
			motion = theme.motion(state.setting),
			dot = 3,
			gap = 1.5,
			w = 64,
			h = 8,
			shrink = 0,
			label = "Drive space",
		},
		ui.text {
			function()
				local x = d()
				return if x then x.free_text or "" else ""
			end,
			font = "$font.mono",
			text_size = 11.5,
		},
	}
end

local function status_bar(): Node
	local function has_drive(): boolean
		return current_drive:get() ~= nil
	end
	return ui.row {
		h = 28,
		w_full = true,
		pl = 14,
		pr = 6,
		gap = 6,
		items = "center",
		border_sides = { 1, 0, 0, 0 },
		border_color = "$border.subtle",
		ui.row {
			grow = 1,
			min_w = 60,
			shrink = 1,
			items = "center",
			selection_cell(),
		},
		ui.show(OpsPanel.active, ui.row {
			gap = 6,
			min_w = 0,
			max_w = 520,
			shrink = 100,
			items = "center",
			divider(),
			OpsPanel.cell("folder"),
		}),
		divider(has_drive),
		ui.show(has_drive, space_cell()),
	}
end

-- On the Overview the footer only shows file operations (its cleanup runs one).
local function overview_footer(): Node
	return ui.show(OpsPanel.active, ui.row {
		h = 28,
		w_full = true,
		px = 6,
		items = "center",
		justify = "end",
		border_sides = { 1, 0, 0, 0 },
		border_color = "$border.subtle",
		OpsPanel.cell("overview"),
	})
end

local function is_overview(): boolean
	local n = state.nav:get()
	return n ~= nil and n.overview == true
end

return ui.column {
	w_full = true,
	h_full = true,
	-- The accent, glow and corners from Settings, over the whole window.
	theme = theme.scope(state.setting),
	bg = "$app.chrome",
	on_key = on_key,
	-- Drag-select keeps following the pointer outside the folder view.
	on_pointer_move = function(e)
		local f = state.root_pointer.move
		if f then
			f(e)
		end
	end,
	on_release = function(e)
		local f = state.root_pointer.release
		if f then
			f(e)
		end
	end,
	on_hover = function(inside: boolean)
		local f = state.root_pointer.release
		if not inside and f then
			f(nil)
		end
	end,
	-- The mouse's back and forward buttons.
	on_aux_click = function(e)
		if e.button == 4 then
			ui.invoke("back")
		elseif e.button == 5 then
			ui.invoke("forward")
		end
	end,
	Tabs(),
	ui.column {
		grow = 1,
		w_full = true,
		min_h = 0,
		Toolbar.nav(),
		ui.row {
			grow = 1,
			w_full = true,
			min_h = 0,
			Sidebar(),
			ui.column {
				grow = 1,
				min_w = 0,
				h_full = true,
				Toolbar.commands(),
				ui.column {
					grow = 1,
					w_full = true,
					min_h = 0,
					bg = "$surface.base",
					rounded_tl = "$radius.lg",
					border_sides = { 1, 0, 0, 1 },
					border_color = "$border.subtle",
					inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
					overflow = "hidden",
					relative = true,
					ui.show(is_overview, ui.column {
						grow = 1,
						w_full = true,
						min_h = 0,
						Overview(),
						overview_footer(),
					}, ui.row {
						grow = 1,
						w_full = true,
						min_h = 0,
						ui.column {
							grow = 1,
							min_w = 0,
							h_full = true,
							Views(),
							status_bar(),
						},
						ui.show(function()
							return state.setting("preview_pane") == true
						end, Preview()),
					}),
				},
			},
		},
	},
	customize.sheet(),
	confirm.dialog(),
	update_modal.dialog(),
	about.dialog(),
}
