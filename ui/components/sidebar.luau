-- The sidebar: Overview (the storage home), pinned folders, the user's
-- folders, and drives with mini dot-matrix meters. The current place gets
-- an amber LED pip. Ctrl+click or middle click opens in a new tab.

local state = require("../state")
local theme = require("../theme")
local common = require("./common")
local icons = require("./icons")
local meter = require("./meter")

local function is_here(path: string): () -> boolean
	return function()
		local n = state.nav:get()
		return n ~= nil and string.lower(n.path) == string.lower(path)
	end
end

local function open_place(path: string)
	return function(e)
		if e and e.ctrl then
			ui.invoke("navigate_new_tab", path)
		else
			ui.invoke("navigate", path)
		end
	end
end

-- One row: pip, icon (shell icon or one of Pane's), label.
local function row(props: {
	label: string,
	path: string,
	here: () -> boolean,
	icon: Node,
	extra: Node?,
	menu: (() -> { Node })?,
	h: number?,
}): Node
	return ui.row {
		role = "link",
		label = props.label,
		selected = props.here,
		relative = true,
		w_full = true,
		h = props.h or 32,
		pl = 12,
		pr = 8,
		gap = 10,
		items = "center",
		rounded = "$radius.md",
		bg = function()
			return if props.here() then "$selection.item" else "#00000000"
		end,
		color = function()
			return if props.here() then "$text.primary" else "$text.secondary"
		end,
		hover = { bg = "$hover", color = "$text.primary" },
		on_click = open_place(props.path),
		-- A place takes dropped files (moved within a drive, copied across)
		-- and opens when a drag rests on it.
		drop_when = function(payload: any)
			return type(payload) == "table" and payload.paths ~= nil and string.sub(props.path, 1, 2) ~= "::"
		end,
		on_drop = function(payload: any)
			ui.invoke("drop", { paths = payload.paths, target = props.path, copy = ui.modifiers.control:peek() })
		end,
		on_file_drop = function(files)
			local paths = {}
			for _, f in files do
				table.insert(paths, f.path)
			end
			if string.sub(props.path, 1, 2) ~= "::" then
				ui.invoke("drop", { paths = paths, target = props.path, copy = ui.modifiers.control:peek() })
			end
		end,
		on_drag_spring = function()
			ui.invoke("navigate", props.path)
			return true
		end,
		on_aux_click = function(e)
			if e.button == "middle" then
				ui.invoke("navigate_new_tab", props.path)
			end
		end,
		context_menu = props.menu,
		common.pip(props.here),
		props.icon,
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 4,
			ui.text { props.label, literal = true, truncate = true },
			props.extra,
		},
	}
end

local function place_row(place: state.Place): Node
	local here = is_here(place.path)
	return row {
		label = place.label,
		path = place.path,
		here = here,
		icon = common.file_icon(function()
			return place.icon
		end, function()
			return true
		end, 18),
		menu = function()
			local items = {
				ui.menu_item {
					"Open in new tab",
					on_click = function()
						ui.invoke("navigate_new_tab", place.path)
					end,
				},
			}
			table.insert(
				items,
				if place.pinned
					then ui.menu_item {
						"Unpin from Quick access",
						on_click = function()
							ui.invoke("unpin", place.path)
						end,
					}
					else ui.menu_item {
						"Pin to Quick access",
						on_click = function()
							ui.invoke("pin", place.path)
						end,
					}
			)
			return items
		end,
	}
end

-- A drive's mini meter: its category breakdown when a scan knows it, else
-- just how full it is.
local function drive_data(place: state.Place): () -> meter.MeterData?
	return function()
		local s = state.storage:get()
		local letter = string.sub(place.path, 1, 2)
		if s and s.drives then
			for _, d in s.drives do
				if string.lower(d.letter) == string.lower(letter) and d.categories then
					return { bytes = d.categories, total = d.total }
				end
			end
		end
		return { bytes = { apps = (place.used or 0) }, total = 1 }
	end
end

local function drive_row(place: state.Place): Node
	local here = is_here(place.path)
	return row {
		label = place.label,
		path = place.path,
		here = here,
		h = 50,
		icon = common.file_icon(function()
			return place.icon
		end, function()
			return true
		end, 18),
		extra = ui.column {
			gap = 3,
			w_full = true,
			meter.meter {
				data = drive_data(place),
				glow = function()
					return (state.setting("glow") or 70) / 100 * 0.6
				end,
				motion = theme.motion(state.setting),
				dot = 3,
				gap = 1.5,
				w_full = true,
				h = 8,
				label = `{place.label}: {math.floor((place.used or 0) * 100)}% used`,
			},
			ui.text {
				place.detail or "",
				literal = true,
				font = "$font.mono",
				text_size = 10.5,
				color = "$text.tertiary",
				truncate = true,
			},
		},
	}
end

local function section(title: string, list: () -> { state.Place }, build: (state.Place) -> Node): Node
	local items = ui.computed(list)
	return ui.show(
		function()
			return #items:get() > 0
		end,
		ui.column {
			w_full = true,
			gap = 1,
			pb = 12,
			ui.div { px = 12, pt = 2, pb = 6, common.caption(title) },
			ui.each(items, function(place: state.Place)
				return build(place)
			end),
		}
	)
end

local function places_field(name: string): () -> { state.Place }
	return function()
		local p = state.places:get()
		return if p then (p :: any)[name] else {}
	end
end

return function(): Node
	return ui.scroll {
		w = 240,
		h_full = true,
		shrink = 0,
		px = 8,
		pb = 8,
		gap = 0,
		ui.column {
			w_full = true,
			pb = 12,
			row {
				label = "Overview",
				path = "::overview",
				here = function()
					local n = state.nav:get()
					return n ~= nil and n.overview == true
				end,
				icon = ui.div { color = "$accent", icons.icon("overview", 18) },
			},
		},
		section("Quick access", places_field("pinned"), place_row),
		section("Folders", places_field("known"), place_row),
		ui.column {
			w_full = true,
			gap = 1,
			ui.div { px = 12, pt = 2, pb = 6, common.caption("This PC") },
			row {
				label = "All drives",
				path = "::thispc",
				here = function()
					local n = state.nav:get()
					return n ~= nil and n.this_pc == true
				end,
				icon = ui.div { color = "$text.secondary", icons.icon("computer", 18) },
			},
			ui.each(ui.computed(places_field("drives")), function(place: state.Place)
				return drive_row(place)
			end),
		},
	}
end
