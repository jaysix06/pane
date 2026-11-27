-- The sidebar: Overview (the storage home), pinned folders, the user's
-- folders, and drives with mini dot-matrix meters. The current place gets
-- an amber LED pip. Ctrl+click or middle click opens in a new tab.

local state = require("../state")
local theme = require("../theme")
local common = require("./common")
local icons = require("./icons")
local meter = require("./meter")
local update_modal = require("./update_modal")

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
	-- Shown in place of the plain label (a drive's name and free space).
	title: Node?,
	menu: (() -> { Node })?,
	h: number?,
	-- A row that isn't a place to go (the Recycle Bin): what a click does,
	-- and what files dropped on it do.
	on_open: (() -> ())?,
	on_paths: ((paths: { string }) -> ())?,
}): Node
	local function take(paths: { string })
		if props.on_paths then
			props.on_paths(paths)
		elseif string.sub(props.path, 1, 2) ~= "::" then
			ui.invoke("drop", { paths = paths, target = props.path, copy = ui.modifiers.control:peek() })
		end
	end
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
		on_click = if props.on_open
			then function()
				(props.on_open :: () -> ())()
			end
			else open_place(props.path),
		-- A place takes dropped files (moved within a drive, copied across)
		-- and opens when a drag rests on it.
		drop_when = function(payload: any)
			return type(payload) == "table"
				and payload.paths ~= nil
				and (props.on_paths ~= nil or string.sub(props.path, 1, 2) ~= "::")
		end,
		on_drop = function(payload: any)
			take(payload.paths)
		end,
		on_file_drop = function(files)
			local paths = {}
			for _, f in files do
				table.insert(paths, f.path)
			end
			take(paths)
		end,
		on_drag_spring = function()
			if props.on_open then
				return false
			end
			ui.invoke("navigate", props.path)
			return true
		end,
		on_aux_click = function(e)
			if e.button == "middle" and not props.on_open then
				ui.invoke("navigate_new_tab", props.path)
			end
		end,
		context_menu = props.menu,
		context_menu_style = common.menu_style,
		common.pip(props.here),
		props.icon,
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 4,
			props.title or ui.text { props.label, literal = true, truncate = true },
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
			return {
				common.menu_entry("Open in new tab", function()
					ui.invoke("navigate_new_tab", place.path)
				end, { icon = "plus" }),
				common.menu_line(),
				if place.pinned
					then common.menu_entry("Unpin from Quick access", function()
						ui.invoke("unpin", place.path)
					end, { icon = "pin" })
					else common.menu_entry("Pin to Quick access", function()
						ui.invoke("pin", place.path)
					end, { icon = "pin" }),
			}
		end,
	}
end

-- Whether the folder on screen is on drive `place` (its root or below).
local function on_drive(place: state.Place): () -> boolean
	local letter = string.lower(string.sub(place.path, 1, 2))
	return function()
		local n = state.nav:get()
		return n ~= nil
			and not n.this_pc
			and not n.overview
			and n.path ~= nil
			and string.lower(string.sub(n.path, 1, 2)) == letter
	end
end

-- A drive: its name with the free space right-aligned, a row of LEDs
-- (its category breakdown once the Overview has scanned it), and
-- "352 of 476 GB used". The drive the folder on screen is on gets a
-- brighter name and amber free space.
local function drive_row(place: state.Place): Node
	local here = is_here(place.path)
	local current = on_drive(place)
	return row {
		label = place.label,
		path = place.path,
		here = here,
		h = 58,
		icon = common.file_icon(function()
			return place.icon
		end, function()
			return true
		end, 18),
		title = ui.row {
			w_full = true,
			gap = 8,
			items = "center",
			ui.text {
				place.label,
				literal = true,
				truncate = true,
				grow = 1,
				min_w = 0,
				weight = function()
					return if current() then 600 else 400
				end,
			},
			-- "123 GB" (the line below says "used"): the name keeps its letter.
			ui.text {
				(string.gsub(place.free_text or "", " free$", "")),
				literal = true,
				font = "$font.mono",
				text_size = 10.5,
				shrink = 0,
				color = function()
					return if current() then "$accent" else "$text.tertiary"
				end,
			},
		},
		extra = ui.column {
			gap = 4,
			w_full = true,
			meter.meter {
				data = function()
					return meter.drive_data(state.storage:get(), string.sub(place.path, 1, 2), place.used or 0)
				end,
				glow = function()
					return (state.setting("glow") or 70) / 100 * (if current() then 0.8 else 0.5)
				end,
				motion = theme.motion(state.setting),
				dot = 4.5,
				gap = 2,
				w_full = true,
				h = 6,
				label = `{place.label}: {math.floor((place.used or 0) * 100)}% used`,
			},
			ui.text {
				place.used_of or place.detail or "",
				literal = true,
				font = "$font.mono",
				text_size = 10.5,
				color = "$text.tertiary",
				truncate = true,
			},
		},
	}
end

-- The Recycle Bin: how full it is; a click opens it in a tab (restore or
-- delete for good there), files dropped on it go there.
local function recycle_info(): any
	local p = state.places:get()
	return if p then (p :: any).recycle else nil
end

local function recycle_row(): Node
	return ui.show(
		function()
			return recycle_info() ~= nil
		end,
		ui.column {
			w_full = true,
			pb = 12,
			row {
				label = "Recycle Bin",
				path = "::recycle",
				here = state.in_recycle,
				icon = ui.div { color = "$text.secondary", icons.icon("trash", 18) },
				h = 44,
				extra = ui.text {
					function()
						local r = recycle_info()
						return if r then r.detail else ""
					end,
					font = "$font.mono",
					text_size = 10.5,
					color = "$text.tertiary",
					truncate = true,
				},
				on_open = function()
					ui.invoke("navigate", "::recycle")
				end,
				on_paths = function(paths: { string })
					ui.invoke("recycle", { paths = paths })
				end,
				menu = function()
					local items = {
						common.menu_entry("Open in new tab", function()
							ui.invoke("navigate_new_tab", "::recycle")
						end, { icon = "plus" }),
						common.menu_entry("Open in File Explorer", function()
							ui.invoke("open_recycle_bin")
						end, { icon = "external" }),
					}
					local r = recycle_info()
					if r and r.count > 0 then
						table.insert(items, common.menu_line())
						table.insert(
							items,
							common.menu_entry("Empty Recycle Bin", function()
								ui.invoke("empty_recycle_bin")
							end, { icon = "trash", danger = true })
						)
					end
					return items
				end,
			},
		}
	)
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
		if p == nil or (name == "drives" and p.show_drives == false) then
			return {}
		end
		return (p :: any)[name]
	end
end

-- The update card: anchored at the sidebar's foot in the signature colour
-- while a newer release is out. It follows the update's phase: available
-- ("Get the update" downloads it and shows the dialog), downloading (its
-- progress; click to see the dialog), ready ("Update now"), or failed.
-- Rust: crates/pane-app/src/updates.rs.
local function card_button(label: string, icon: string, on_click: () -> ()): Node
	return ui.button {
		label = label,
		on_click = on_click,
		w_full = true,
		h = 30,
		gap = 6,
		rounded = "$radius.md",
		bg = "#0000002E",
		color = "$accent.text",
		hover = { bg = "#00000047" },
		items = "center",
		justify = "center",
		icons.icon(icon, 14, "$accent.text"),
		ui.text { label, weight = 600, text_size = 12.5 },
	}
end

local function update_card(): Node
	local function u(): any
		return state.update:get() or {}
	end
	local function phase(): string
		return u().phase or "available"
	end
	local titles = {
		available = "Update available",
		downloading = "Downloading update",
		ready = "Update ready",
		error = "Update failed",
	}
	return ui.show(
		function()
			return state.update:get() ~= nil
		end,
		ui.column {
			w_full = true,
			p = 12,
			gap = 10,
			rounded = "$radius.lg",
			bg = "$accent",
			color = "$accent.text",
			shadow = { x = 0, y = 6, blur = 22, spread = 0, color = "$app.glow" },
			inset_shadow = { x = 0, y = 1, color = "#FFFFFF33" },
			role = "status",
			label = function()
				return `{titles[phase()] or ""}: Pane {u().version or ""}`
			end,
			on_mount = function(el)
				if theme.motion(state.setting)() then
					anime.animate(el, { opacity = { 0, 1 }, translateY = { 12, 0 }, duration = 420, ease = "out(4)" })
				end
			end,
			ui.row {
				w_full = true,
				gap = 8,
				items = "center",
				ui.switch(phase, {
					available = icons.icon("sparkle", 16, "$accent.text"),
					downloading = icons.icon("download", 16, "$accent.text"),
					ready = icons.icon("check", 16, "$accent.text"),
					error = icons.icon("warning", 16, "$accent.text"),
				}),
				ui.text {
					function()
						return titles[phase()] or ""
					end,
					weight = 700,
					grow = 1,
				},
				-- Only an update that is merely available can be put off.
				ui.show(
					function()
						return phase() == "available"
					end,
					ui.button {
						label = "Later",
						tooltip = "Not now (hides this version)",
						on_click = ui.action("update_dismiss"),
						w = 22,
						h = 22,
						p = 0,
						rounded = "$radius.sm",
						bg = "#00000000",
						color = "$accent.text",
						hover = { bg = "#0000001F" },
						items = "center",
						justify = "center",
						icons.icon("close", 12, "$accent.text"),
					}
				),
			},
			ui.text {
				function()
					local x = u()
					local p = phase()
					if p == "downloading" then
						return `{x.done_text or ""} of {x.total_text or ""}`
					elseif p == "ready" then
						return `Pane {x.version} is downloaded. Restart to finish.`
					elseif p == "error" then
						return "The download didn't finish."
					end
					return `Pane {x.version} is out. You have {x.current}.`
				end,
				text_size = 12,
				wrap = true,
				opacity = 0.85,
			},
			ui.show(
				function()
					return phase() == "downloading"
				end,
				ui.div {
					w_full = true,
					h = 4,
					rounded = "$radius.full",
					bg = "#00000033",
					overflow = "hidden",
					ui.div {
						h_full = true,
						w = function()
							return `{math.max(2, math.floor((u().progress or 0) * 100))}%`
						end,
						rounded = "$radius.full",
						bg = "$accent.text",
					},
				}
			),
			ui.switch(phase, {
				available = card_button("Get the update", "download", update_modal.start),
				downloading = card_button("Show progress", "download", function()
					update_modal.open:set(true)
				end),
				ready = card_button("Update now", "refresh", function()
					ui.invoke("update_install")
				end),
				error = card_button("Try again", "refresh", update_modal.start),
			}),
		}
	)
end

return function(): Node
	return ui.column {
		w = 240,
		h_full = true,
		shrink = 0,
		ui.scroll {
		w_full = true,
		grow = 1,
		min_h = 0,
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
		recycle_row(),
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
	},
		ui.div { w_full = true, px = 8, pb = 8, update_card() },
	}
end
