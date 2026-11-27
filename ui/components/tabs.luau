-- The tab strip, in the title bar: Pane's mark, the tabs (the active one a
-- raised card with a glowing amber underline), a new-tab button, and the
-- rest of the strip as a window drag region.

local state = require("../state")
local common = require("./common")
local icons = require("./icons")

-- Where the tab with `id` is in the strip now (1-based), or nil. A row is
-- kept while its tab stays open, so the index it was built with goes stale
-- when a tab before it closes: look it up when it's needed.
local function index_of(id: number): number?
	for i, t in state.tabs:get() do
		if t.id == id then
			return i
		end
	end
	return nil
end

local function tab_view(tab: state.Tab): Node
	local active = ui.computed(function()
		return index_of(tab.id) == state.active:get()
	end)
	-- Runs `action` with this tab's current index.
	local function on_tab(action: string): () -> ()
		return function()
			local i = index_of(tab.id)
			if i then
				ui.invoke(action, i)
			end
		end
	end
	local overview = tab.path == "::overview"
	return ui.row {
		role = "tab",
		label = tab.title,
		selected = function()
			return active:get()
		end,
		relative = true,
		h = 34,
		mt = 6,
		pl = 10,
		pr = 4,
		gap = 8,
		items = "center",
		w = 210,
		min_w = 96,
		shrink = 1,
		rounded_tl = "$radius.md",
		rounded_tr = "$radius.md",
		bg = function()
			return if active:get() then "$surface.base" else "#00000000"
		end,
		color = function()
			return if active:get() then "$text.primary" else "$text.secondary"
		end,
		hover = { bg = "$hover" },
		on_click = on_tab("switch_tab"),
		-- Middle click closes, as in a browser.
		on_aux_click = function(e)
			if e.button == "middle" then
				on_tab("close_tab")()
			end
		end,
		ui.div {
			color = function()
				return if active:get() then "$accent" else "$text.tertiary"
			end,
			icons.icon(if overview then "overview" else "folder", 15),
		},
		ui.text { tab.title, literal = true, truncate = true, grow = 1, min_w = 0 },
		ui.show(tab.loading, ui.div { w = 6, h = 6, rounded = "$radius.full", bg = "$accent", opacity = 0.8 }),
		ui.button {
			label = "Close tab",
			tooltip = "Close tab (Ctrl+W)",
			on_click = on_tab("close_tab"),
			w = 22,
			h = 22,
			p = 0,
			rounded = "$radius.sm",
			bg = "#00000000",
			color = "$text.tertiary",
			hover = { bg = "$hover", color = "$text.primary" },
			items = "center",
			justify = "center",
			shrink = 0,
			icons.icon("close", 12),
		},
		-- The active tab's underline: a short amber LED bar.
		ui.div {
			absolute = true,
			left = 12,
			right = 12,
			bottom = 0,
			h = 2,
			rounded = "$radius.full",
			bg = "$accent",
			shadow = { x = 0, y = 0, blur = 10, spread = 0, color = "$app.glow" },
			opacity = ui.tween(active, { duration = 180 }),
		},
	}
end

-- Pane's mark: a small dot-matrix "P" glyph in a rounded tile.
local function mark(): Node
	return ui.row {
		h = 40,
		pl = 6,
		pr = 4,
		gap = 8,
		items = "center",
		shrink = 0,
		ui.div {
			w = 22,
			h = 22,
			rounded = "$radius.md",
			bg = "$accent",
			shadow = { x = 0, y = 0, blur = 14, spread = 0, color = "$app.glow" },
			items = "center",
			justify = "center",
			ui.svg {
				code = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'>"
					.. "<g fill='white'>"
					.. "<circle cx='3' cy='2' r='1'/><circle cx='6' cy='2' r='1'/><circle cx='9' cy='3.2' r='1'/>"
					.. "<circle cx='3' cy='5' r='1'/><circle cx='9' cy='5.8' r='1'/>"
					.. "<circle cx='3' cy='8' r='1'/><circle cx='6' cy='7' r='1'/>"
					.. "<circle cx='3' cy='11' r='1'/>"
					.. "</g></svg>",
				w = 12,
				h = 12,
				tint = "$accent.text",
				aria_hidden = true,
			},
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
		mark(),
		ui.row {
			shrink = 1,
			min_w = 0,
			gap = 2,
			items = "end",
			h_full = true,
			ui.each(state.tabs, function(tab: state.Tab)
				return tab_view(tab)
			end, {
				key = function(tab: state.Tab)
					return tab.id
				end,
			}),
		},
		ui.row {
			shrink = 0,
			h = 40,
			items = "center",
			pl = 4,
			common.icon_button { label = "New tab (Ctrl+T)", icon = "plus", on_click = ui.action("new_tab"), size = 28 },
		},
		-- The rest of the strip: empty, so it drags the window.
		ui.div { grow = 1, h_full = true },
	}
end
