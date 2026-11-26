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

local function status_bar(): Node
	return ui.row {
		h = 28,
		w_full = true,
		px = 14,
		items = "center",
		border_sides = { 1, 0, 0, 0 },
		border_color = "$border.subtle",
		ui.text {
			function()
				return state.status:get()
			end,
			color = "$text.tertiary",
			font = "$font.mono",
			text_size = 11.5,
			truncate = true,
		},
	}
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
		Toolbar(),
		ui.row {
			grow = 1,
			w_full = true,
			min_h = 0,
			Sidebar(),
			ui.column {
				grow = 1,
				min_w = 0,
				h_full = true,
				bg = "$surface.base",
				rounded_tl = "$radius.lg",
				border_sides = { 1, 0, 0, 1 },
				border_color = "$border.subtle",
				inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
				overflow = "hidden",
				relative = true,
				ui.show(is_overview, Overview(), ui.row {
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
				OpsPanel(),
			},
		},
	},
	customize.sheet(),
	confirm.dialog(),
	update_modal.dialog(),
	about.dialog(),
}
