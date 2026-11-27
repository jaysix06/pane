-- The toolbar, in two rows. A floating capsule for getting about: back /
-- forward / up / refresh, the path pill (mono breadcrumbs, or a field while
-- editing), search and More. Under it, a command row that follows the
-- selection: with nothing selected, New folder and Paste; with a selection,
-- a chip saying what's selected and its commands, labelled. Sort, the view
-- switcher and the preview toggle sit at its right.

local state = require("../state")
local common = require("./common")
local icons = require("./icons")
local customize = require("./customize")
local commands = require("./commands")
local about = require("./about")
local shortcuts = require("../shortcuts")

local function nav_flag(name: string): () -> boolean
	return function()
		local n = state.nav:get()
		return n ~= nil and (n :: any)[name] == true
	end
end

local function start_editing()
	local n = state.nav:peek()
	if n and n.overview then
		return
	end
	state.address_text:set(if n then n.address else "")
	state.editing_address:set(true)
end

-- Long paths show their last parts; the rest fold into a "…" menu.
local SHOWN_CRUMBS = 3

local function hidden_crumbs_menu(hidden: any): Node
	local open = ui.state("crumbs_menu_open", false)
	return ui.show(
		function()
			return #hidden:get() > 0
		end,
		ui.menu {
			open = open,
			label = "Earlier folders",
			anchor = ui.button {
				label = "Earlier folders",
				on_click = ui.toggle(open),
				px = 6,
				py = 3,
				rounded = "$radius.sm",
				bg = "#00000000",
				color = "$text.secondary",
				hover = { bg = "$hover", color = "$text.primary" },
				ui.text { "…", font = "$font.mono" },
			},
			ui.each(hidden, function(crumb: state.Crumb)
				return ui.menu_item {
					label = crumb.label,
					on_click = function()
						ui.invoke("navigate", crumb.path)
					end,
				}
			end),
		}
	)
end

local function crumbs_view(): Node
	local all = ui.computed(function(): { state.Crumb }
		local n = state.nav:get()
		return if n then n.crumbs else {}
	end)
	local crumbs = ui.computed(function(): { state.Crumb }
		local c = all:get()
		if #c <= SHOWN_CRUMBS + 1 then
			return c
		end
		return { table.unpack(c, #c - SHOWN_CRUMBS + 1) }
	end)
	local hidden = ui.computed(function(): { state.Crumb }
		local c = all:get()
		if #c <= SHOWN_CRUMBS + 1 then
			return {}
		end
		return { table.unpack(c, 1, #c - SHOWN_CRUMBS) }
	end)
	return ui.row {
		grow = 1,
		min_w = 0,
		h_full = true,
		items = "center",
		overflow = "hidden",
		ui.row {
			shrink = 1,
			min_w = 0,
			items = "center",
			hidden_crumbs_menu(hidden),
			ui.each(crumbs, function(crumb: state.Crumb, index: number)
				local last = function()
					return index == #crumbs:get()
				end
				return ui.row {
					items = "center",
					shrink = 0,
					ui.show(function()
						return index > 1 or #hidden:get() > 0
					end, ui.div { px = 1, color = "$text.tertiary", icons.icon("chevron", 12) }),
					ui.button {
						label = crumb.label,
						on_click = ui.action("navigate", crumb.path),
				-- A place takes dropped files (moved within a drive, copied across)
				-- and opens when a drag rests on it.
				drop_when = function(payload: any)
					return type(payload) == "table" and payload.paths ~= nil and string.sub(crumb.path, 1, 2) ~= "::"
				end,
				on_drop = function(payload: any)
					ui.invoke("drop", { paths = payload.paths, target = crumb.path, copy = ui.modifiers.control:peek() })
				end,
				on_file_drop = function(files)
					local paths = {}
					for _, f in files do
						table.insert(paths, f.path)
					end
					if string.sub(crumb.path, 1, 2) ~= "::" then
						ui.invoke("drop", { paths = paths, target = crumb.path, copy = ui.modifiers.control:peek() })
					end
				end,
				on_drag_spring = function()
					ui.invoke("navigate", crumb.path)
					return true
				end,
						on_aux_click = function(e)
							if e.button == "middle" then
								ui.invoke("navigate_new_tab", crumb.path)
							end
						end,
						px = 6,
						py = 3,
						rounded = "$radius.sm",
						bg = "#00000000",
						color = function()
							return if last() then "$text.primary" else "$text.secondary"
						end,
						hover = { bg = "$hover", color = "$text.primary" },
						ui.text { crumb.label, literal = true, font = "$font.mono", text_size = 12.5 },
					},
				}
			end),
		},
		-- Clicking past the crumbs edits the path, as in Explorer.
		ui.div { grow = 1, h_full = true, min_w = 24, on_click = start_editing },
	}
end

local function address_input(): Node
	return ui.input {
		value = state.address_text,
		label = "Address",
		select_on_focus = true,
		w_full = true,
		h = 30,
		bg = "#00000000",
		px = 6,
		font = "$font.mono",
		text_size = 12.5,
		caret_color = "$accent",
		-- The path pill around it draws the border and the focus.
		border = 0,
		focus = { border = 0 },
		on_mount = function(el)
			el:focus()
		end,
		on_commit = function(text: string)
			if state.editing_address:peek() then
				state.editing_address:set(false)
				ui.invoke("address", text)
			end
		end,
		on_blur = function()
			state.editing_address:set(false)
		end,
		on_key = function(e)
			if e.key == "Escape" then
				state.editing_address:set(false)
				return true
			end
			return false
		end,
	}
end

-- Search this folder and its subfolders (Ctrl+F): Enter searches, Escape
-- clears and goes back to the folder.
local function search_box(): Node
	-- Show the query of the results on screen; empty in a folder.
	ui.effect(function()
		local n = state.nav:get()
		state.search_text:set(if n and n.search then n.search else "")
	end)
	local focused = ui.state("search_focused", false)
	return ui.row {
		w = 280,
		shrink = 0,
		h = 34,
		pl = 10,
		pr = 4,
		gap = 6,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = function()
			local n = state.nav:get()
			return if focused:get() or (n and n.search) then "$accent" else "$border.subtle"
		end,
		color = "$text.tertiary",
		icons.icon("search", 14),
		ui.input {
			ref = state.search_ref,
			value = state.search_text,
			label = "Search this folder",
			placeholder = function()
				local n = state.nav:get()
				if n and n.crumbs and #n.crumbs > 0 and not n.search then
					return `Search {n.crumbs[#n.crumbs].label} and subfolders`
				end
				return "Search"
			end,
			select_on_focus = true,
			grow = 1,
			min_w = 0,
			h = 30,
			px = 2,
			bg = "#00000000",
			text_size = 13,
			caret_color = "$accent",
			-- The box around it draws the border, amber while focused.
			border = 0,
			focus = { border = 0 },
			on_focus = function()
				focused:set(true)
			end,
			on_blur = function()
				focused:set(false)
			end,
			on_key = function(e)
				if e.key == "Enter" then
					ui.invoke("search", state.search_text:peek())
					return true
				elseif e.key == "Escape" then
					state.search_text:set("")
					local n = state.nav:peek()
					if n and n.search then
						ui.invoke("search", "")
					end
					return true
				end
				return false
			end,
		},
		ui.show(function()
			return state.search_text:get() ~= ""
		end, ui.button {
			label = "Clear search",
			on_click = function()
				state.search_text:set("")
				local n = state.nav:peek()
				if n and n.search then
					ui.invoke("search", "")
				end
			end,
			w = 24,
			h = 24,
			p = 0,
			rounded = "$radius.sm",
			bg = "#00000000",
			color = "$text.tertiary",
			hover = { bg = "$hover", color = "$text.primary" },
			items = "center",
			justify = "center",
			icons.icon("close", 12),
		}),
	}
end

local function path_pill(): Node
	return ui.row {
		grow = 1,
		min_w = 0,
		h = 34,
		pl = 10,
		pr = 4,
		gap = 6,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = function()
			return if state.editing_address:get() then "$accent" else "$border.subtle"
		end,
		shadow = function()
			return if state.editing_address:get()
				then { x = 0, y = 0, blur = 12, spread = 0, color = "$app.glow" }
				else false
		end,
		ui.div {
			color = "$accent",
			ui.switch(function()
				local n = state.nav:get()
				return if n and n.overview
					then "overview"
					elseif n and n.this_pc then "computer"
					elseif n and n.recycle then "trash"
					else "folder"
			end, {
				trash = icons.icon("trash", 15),
				overview = icons.icon("overview", 15),
				computer = icons.icon("computer", 15),
				folder = icons.icon("folder", 15),
			}),
		},
		ui.switch(function()
			return if state.editing_address:get() then "edit" else "crumbs"
		end, {
			edit = address_input(),
			crumbs = crumbs_view(),
		}),
	}
end

-- Details / List / Grid as three icon buttons in a well.
local function view_switch(): Node
	local function view_button(view: string, icon: string, label: string): Node
		return common.icon_button {
			label = label,
			icon = icon,
			size = 28,
			on_click = function()
				ui.invoke("set_view", view)
			end,
			active = function()
				return state.view() == view
			end,
		}
	end
	return ui.row {
		gap = 2,
		p = 2,
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = "$border.subtle",
		role = "radiogroup",
		label = "Layout",
		view_button("details", "details", "Details"),
		view_button("list", "list", "List"),
		view_button("grid", "grid", "Grid"),
	}
end

-- A button with an icon and a word: the command row's buttons. `primary`
-- is the amber one; `danger` tints the icon.
local function text_button(props: {
	label: string | () -> string,
	name: string?,
	icon: string,
	on_click: () -> (),
	tooltip: (() -> string)?,
	primary: boolean?,
	danger: boolean?,
}): Node
	local primary = props.primary == true
	return ui.button {
		label = props.name or props.label,
		-- A node, so a rebound key shows without a reload.
		tooltip = if props.tooltip then ui.text { props.tooltip } else nil,
		tooltip_placement = { side = "bottom", align = "center", offset = 6 },
		on_click = props.on_click,
		h = 30,
		pl = 9,
		pr = 11,
		gap = 7,
		items = "center",
		shrink = 0,
		rounded = "$radius.md",
		bg = if primary then "$accent" else "#00000000",
		color = if primary then "$accent.text" else "$text.primary",
		hover = if primary then { bg = "$accent.hover" } else { bg = "$hover" },
		active = { bg = if primary then "$accent" else "$pressed" },
		ui.div {
			color = if primary then "$accent.text" elseif props.danger then "$danger" else "$text.secondary",
			icons.icon(props.icon, 15),
		},
		ui.text { props.label, text_size = 13, weight = if primary then 600 else 400 },
	}
end

local function hint(id: string): () -> string
	return function()
		return shortcuts.hint(id)
	end
end

-- What's selected, after its count: "files · 13 KB".
local function selection_words(): string
	local sel = state.sel:get()
	local files, folders = sel.files or 0, sel.folders or 0
	local one = sel.count == 1
	local noun = if folders == 0
		then (if one then "file" else "files")
		elseif files == 0 then (if one then "folder" else "folders")
		else "items"
	local size = sel.size or ""
	return if size ~= "" then `{noun} · {size}` else noun
end

-- The selection as a chip: its count in amber, what and how big, and an
-- × that clears it (as Esc does).
local function selection_chip(): Node
	return ui.row {
		h = 30,
		pl = 10,
		pr = 3,
		gap = 5,
		items = "center",
		shrink = 0,
		rounded = "$radius.md",
		bg = "$selection.item",
		border = 1,
		border_color = "$selection",
		ui.text {
			function()
				return tostring(state.sel:get().count)
			end,
			font = "$font.mono",
			text_size = 12,
			weight = 600,
			color = "$accent",
		},
		ui.text { selection_words, font = "$font.mono", text_size = 12, color = "$text.primary" },
		ui.button {
			label = "Select none",
			tooltip = "Select none (Esc)",
			tooltip_placement = { side = "bottom", align = "center", offset = 6 },
			on_click = ui.action("select_none"),
			w = 22,
			h = 22,
			p = 0,
			ml = 2,
			rounded = "$radius.sm",
			bg = "#00000000",
			color = "$text.secondary",
			hover = { bg = "$hover", color = "$text.primary" },
			items = "center",
			justify = "center",
			icons.icon("close", 12),
		},
	}
end

-- Commands for what's selected. Nothing on the row is ever disabled: what
-- doesn't apply isn't shown.
local function selection_commands(): Node
	return ui.row {
		gap = 2,
		items = "center",
		selection_chip(),
		ui.div { w = 1, h = 18, bg = "$border.subtle", mx = 6 },
		text_button {
			label = "Open",
			icon = "external",
			tooltip = function()
				return "Open (Enter)"
			end,
			on_click = function()
				ui.invoke("open_selection")
			end,
		},
		text_button { label = "Copy", icon = "copy", tooltip = hint("copy"), on_click = commands.copy },
		text_button { label = "Cut", icon = "cut", tooltip = hint("cut"), on_click = commands.cut },
		ui.show(function()
			return state.sel:get().count == 1
		end, text_button { label = "Rename", icon = "rename", tooltip = hint("rename"), on_click = commands.rename }),
		text_button {
			label = "Delete",
			icon = "trash",
			danger = true,
			tooltip = function()
				return shortcuts.hint("delete") .. ". Hold Shift to delete permanently"
			end,
			on_click = function()
				commands.delete(ui.modifiers.shift:peek())
			end,
		},
	}
end

-- Everything to make in the folder: a folder (Ctrl+Shift+N), or a file of
-- a few kinds. On the command row's right, beside Sort, in folders only.
local function new_menu(): Node
	local open = ui.state("new_menu_open", false)
	local function item(label: string, icon: string, run: () -> (), key: (() -> string)?): Node
		return ui.menu_item {
			label = label,
			on_click = function()
				open:set(false)
				run()
			end,
			ui.row {
				gap = 8,
				items = "center",
				w_full = true,
				ui.div { color = "$text.secondary", icons.icon(icon, 15) },
				ui.text { label, grow = 1 },
				if key
					then ui.text { key, font = "$font.mono", text_size = 11, color = "$text.tertiary", pl = 16 }
					else nil,
			},
		}
	end
	return ui.menu {
		open = open,
		label = "New",
		anchor = ui.button {
			label = "New",
			tooltip = ui.text { "New folder or file" },
			tooltip_placement = { side = "bottom", align = "center", offset = 6 },
			on_click = ui.toggle(open),
			h = 30,
			px = 9,
			gap = 6,
			items = "center",
			shrink = 0,
			rounded = "$radius.md",
			bg = "#00000000",
			color = "$text.primary",
			hover = { bg = "$hover" },
			active = { bg = "$pressed" },
			ui.div { color = "$text.secondary", icons.icon("plus", 15) },
			ui.text { "New", text_size = 13 },
		},
		item("Folder", "folder_plus", commands.new_folder, hint("new_folder")),
		item("Text document", "doc", function()
			commands.new_file("text")
		end),
		item("Markdown document", "doc", function()
			commands.new_file("markdown")
		end),
	}
end

-- Commands for the folder itself, with nothing selected: pasting what's
-- on the clipboard (only while it holds files). New is on the right.
local function folder_commands(): Node
	return ui.row {
		gap = 6,
		items = "center",
		ui.show(
			function()
				return state.clipboard:get().count > 0
			end,
			text_button {
				name = "Paste",
				label = function()
					local c = state.clipboard:get()
					local what = if c.count == 1 then "1 item" else `{c.count} items`
					return if c.cut then `Move {what} here` else `Paste {what}`
				end,
				icon = "paste",
				tooltip = hint("paste"),
				on_click = commands.paste,
			}
		),
	}
end

-- In the Recycle Bin: Restore and Delete permanently for the selection
-- (Empty Recycle Bin is on the right).
local function recycle_commands(): Node
	return ui.row {
		gap = 6,
		items = "center",
		ui.show(function()
			return state.sel:get().count > 0
		end, ui.row {
			gap = 2,
			items = "center",
			selection_chip(),
			ui.div { w = 1, h = 18, bg = "$border.subtle", mx = 6 },
			text_button {
				label = "Restore",
				icon = "back",
				primary = true,
				tooltip = function()
					return "Put back where it was deleted from"
				end,
				on_click = commands.restore,
			},
			text_button {
				label = "Delete permanently",
				icon = "trash",
				danger = true,
				tooltip = function()
					return "Delete for good (Delete)"
				end,
				on_click = commands.purge,
			},
		}),
	}
end

-- The Recycle Bin's own command, on the command row's right beside Sort
-- (nothing goes above the sidebar): while it holds anything and nothing is
-- selected.
local function empty_bin_button(): Node
	return ui.show(function()
		local n = state.nav:get()
		return n ~= nil and n.recycle == true and state.sel:get().count == 0 and state.rows:len() > 0
	end, text_button {
		label = "Empty Recycle Bin",
		icon = "trash",
		danger = true,
		tooltip = function()
			return "Delete everything in it for good"
		end,
		on_click = commands.empty_bin,
	})
end

-- The command row's left side: the selection's commands, or the folder's.
local function command_row_left(): Node
	return ui.switch(function()
		local n = state.nav:get()
		if n == nil or n.this_pc then
			return "none"
		elseif n.recycle then
			return "recycle"
		elseif state.sel:get().count > 0 then
			return "selection"
		elseif n.search then
			-- Search results aren't a folder: nothing to make or paste there.
			return "none"
		end
		return "folder"
	end, {
		selection = selection_commands(),
		folder = folder_commands(),
		recycle = recycle_commands(),
		none = ui.div {},
	})
end

local function sort_menu(): Node
	local open = ui.state("sort_menu_open", false)
	local function sort_is(key: string): () -> boolean
		return function()
			local n = state.nav:get()
			return n ~= nil and n.sort == key
		end
	end
	local function by(key: string): () -> ()
		return function()
			ui.invoke("sort", key)
		end
	end
	local function direction(descending: boolean): () -> boolean
		return function()
			local n = state.nav:get()
			return n ~= nil and n.descending == descending
		end
	end
	local function flip_to(descending: boolean): () -> ()
		return function()
			local n = state.nav:peek()
			if n and n.descending ~= descending then
				ui.invoke("sort", n.sort)
			end
		end
	end
	return ui.menu {
		open = open,
		label = "Sort",
		-- Says what it sorts by, so the order on screen is never a guess.
		anchor = ui.button {
			label = "Sort",
			on_click = ui.toggle(open),
			h = 30,
			px = 9,
			gap = 6,
			items = "center",
			shrink = 0,
			rounded = "$radius.md",
			bg = "#00000000",
			color = "$text.secondary",
			hover = { bg = "$hover", color = "$text.primary" },
			active = { bg = "$pressed" },
			icons.icon("sort", 15),
			ui.text { "Sort", text_size = 12.5 },
			ui.text {
				function()
					local n = state.nav:get()
					local key = if n then n.sort else "name"
					return ({ name = "Name", date = "Date", type = "Type", size = "Size" } :: { [string]: string })[key] or "Name"
				end,
				text_size = 12.5,
				weight = 600,
				color = "$text.primary",
			},
		},
		common.check_item("Name", sort_is("name"), by("name")),
		common.check_item("Date modified", sort_is("date"), by("date")),
		common.check_item("Type", sort_is("type"), by("type")),
		common.check_item("Size", sort_is("size"), by("size")),
		common.separator(),
		common.check_item("Ascending", direction(false), flip_to(false)),
		common.check_item("Descending", direction(true), flip_to(true)),
	}
end

local function more_menu(): Node
	local open = ui.state("more_menu_open", false)
	local function toggle_setting(name: string): () -> ()
		return function()
			ui.invoke("set_setting", { key = name, value = not state.setting(name) })
		end
	end
	local function setting_on(name: string): () -> boolean
		return function()
			return state.setting(name) == true
		end
	end
	return ui.menu {
		open = open,
		label = "More",
		anchor = common.icon_button { label = "More", icon = "more", on_click = ui.toggle(open) },
		common.menu_item("Customize look…", function()
			customize.open:set(true)
		end, "palette"),
		common.separator(),
		common.check_item("Hidden items", setting_on("show_hidden"), toggle_setting("show_hidden")),
		common.check_item("File name extensions", setting_on("show_extensions"), toggle_setting("show_extensions")),
		common.check_item("Folders first", setting_on("folders_first"), toggle_setting("folders_first")),
		common.separator(),
		common.menu_item("Pin to Quick access", function()
			ui.invoke("pin_current")
		end, "pin"),
		common.menu_item("Select all", function()
			ui.invoke("select_all")
		end, "check"),
		ui.show(
			function()
				local s = state.storage:get()
				return s ~= nil and s.instant == true
			end,
			common.menu_item("Turn off instant scan…", function()
				ui.invoke("remove_instant_scan")
			end, "bolt")
		),
		common.separator(),
		common.menu_item("Settings (Ctrl+,)", function()
			shortcuts.open_settings()
		end, "settings"),
		common.menu_item("About Pane", about.show, "info"),
	}
end

return function(): Node
	return ui.column {
		w_full = true,
		px = 8,
		pt = 2,
		pb = 6,
		gap = 6,
		-- Getting about: navigation, the path, search, and More.
		ui.row {
			w_full = true,
			h = 46,
			px = 6,
			gap = 4,
			items = "center",
			rounded = "$radius.lg",
			bg = "$surface.base",
			border = 1,
			border_color = "$border.subtle",
			inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
			shadow = "$shadow.sm",
			common.icon_button { label = "Back (Alt+Left)", icon = "back", on_click = ui.action("back"), enabled = nav_flag("can_back") },
			common.icon_button {
				label = "Forward (Alt+Right)",
				icon = "forward",
				on_click = ui.action("forward"),
				enabled = nav_flag("can_forward"),
			},
			common.icon_button { label = "Up (Alt+Up)", icon = "up", on_click = ui.action("up"), enabled = nav_flag("can_up") },
			common.icon_button { label = "Refresh (F5)", icon = "refresh", on_click = ui.action("refresh") },
			ui.div { w = 4 },
			path_pill(),
			ui.show(function()
				local n = state.nav:get()
				return n ~= nil and not n.overview and not n.this_pc and not n.recycle
			end, search_box()),
			more_menu(),
		},
		-- Acting: commands for the selection (or the folder) on the left,
		-- how the folder shows on the right.
		ui.show(
			function()
				local n = state.nav:get()
				return n ~= nil and not n.overview
			end,
			ui.row {
				w_full = true,
				h = 34,
				px = 4,
				gap = 4,
				items = "center",
				command_row_left(),
				ui.div { grow = 1, min_w = 8 },
				-- New: in a folder (not This PC or search results).
				ui.show(function()
					local n = state.nav:get()
					return n ~= nil and not n.this_pc and not n.search and not n.recycle
				end, new_menu()),
				empty_bin_button(),
				sort_menu(),
				view_switch(),
				common.icon_button {
					label = "Preview pane (Alt+P)",
					icon = "preview",
					on_click = function()
						ui.invoke("set_setting", { key = "preview_pane", value = not state.setting("preview_pane") })
					end,
					active = function()
						return state.setting("preview_pane") == true
					end,
				},
			}
		),
	}
end
