-- The tab strip, in the title bar: tabs, a new-tab button, and the rest of
-- the strip as a window drag region.

local state = require("../state")
local common = require("./common")

local function tab_view(tab: state.Tab, index: number): Node
	local function is_active(): boolean
		return state.active:get() == index
	end
	return ui.row {
		role = "tab",
		label = tab.title,
		selected = is_active,
		h = 34,
		mt = 6,
		pl = 12,
		pr = 4,
		gap = 6,
		items = "center",
		w = 200,
		min_w = 90,
		shrink = 1,
		rounded_tl = 8,
		rounded_tr = 8,
		bg = function()
			return if is_active() then "$surface.raised" else "#00000000"
		end,
		hover = { bg = "$hover" },
		on_click = ui.action("switch_tab", index),
		-- Middle click closes, as in a browser.
		on_aux_click = function(e)
			if e.button == "middle" then
				ui.invoke("close_tab", index)
			end
		end,
		ui.text {
			function()
				return if tab.loading then "◌" else ""
			end,
			text_size = 11,
			color = "$text.tertiary",
		},
		ui.text { tab.title, literal = true, truncate = true, grow = 1, min_w = 0 },
		ui.button {
			label = "Close tab",
			tooltip = "Close tab (Ctrl+W)",
			on_click = ui.action("close_tab", index),
			w = 24,
			h = 24,
			p = 0,
			rounded = 4,
			bg = "#00000000",
			hover = { bg = "$hover" },
			items = "center",
			justify = "center",
			shrink = 0,
			ui.text { "✕", text_size = 10, color = "$text.secondary" },
		},
	}
end

return function(): Node
	return ui.row {
		role = "tablist",
		h = 40,
		w_full = true,
		window_drag = true,
		items = "end",
		gap = 2,
		pl = 8,
		-- Clear of Windows' minimize / maximize / close at the top right.
		pr = function()
			local area, buttons = ui.window.titlebar_area:get(), ui.window.controls:get()
			return if area and buttons and buttons.x >= area.x + area.width then buttons.width + 16 else 16
		end,
		ui.row {
			shrink = 1,
			min_w = 0,
			gap = 2,
			items = "end",
			h_full = true,
			ui.each(state.tabs, function(tab: state.Tab, index: number)
				return tab_view(tab, index)
			end),
		},
		ui.row {
			shrink = 0,
			h = 40,
			items = "center",
			pl = 4,
			common.icon_button {
				label = "New tab (Ctrl+T)",
				glyph = "+",
				on_click = ui.action("new_tab"),
				size = 28,
			},
		},
		-- The rest of the strip: empty, so it drags the window.
		ui.div { grow = 1, h_full = true },
	}
end
