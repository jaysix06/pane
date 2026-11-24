-- The toolbar: a floating capsule with back / forward / up / refresh, the
-- path pill (mono breadcrumbs, or a field while editing), the view
-- switcher, and the Sort and More menus.

local state = require("../state")
local common = require("./common")
local icons = require("./icons")
local customize = require("./customize")

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

local function crumbs_view(): Node
	local crumbs = ui.computed(function(): { state.Crumb }
		local n = state.nav:get()
		return if n then n.crumbs else {}
	end)
	return ui.row {
		grow = 1,
		min_w = 0,
		h_full = true,
		items = "center",
		-- The end of the path matters most: overflow is cut at the start.
		justify = "end",
		overflow = "hidden",
		ui.row {
			shrink = 0,
			items = "center",
			ui.each(crumbs, function(crumb: state.Crumb, index: number)
				local last = function()
					return index == #crumbs:get()
				end
				return ui.row {
					items = "center",
					shrink = 0,
					ui.show(index > 1, ui.div { px = 1, color = "$text.tertiary", icons.icon("chevron", 12) }),
					ui.button {
						label = crumb.label,
						on_click = ui.action("navigate", crumb.path),
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
				return if n and n.overview then "overview" elseif n and n.this_pc then "computer" else "folder"
			end, {
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
		anchor = common.icon_button { label = "Sort", icon = "sort", on_click = ui.toggle(open) },
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
		ui.menu_item {
			"Customize look…",
			on_click = function()
				customize.open:set(true)
			end,
		},
		common.separator(),
		common.check_item("Hidden items", setting_on("show_hidden"), toggle_setting("show_hidden")),
		common.check_item("File name extensions", setting_on("show_extensions"), toggle_setting("show_extensions")),
		common.check_item("Folders first", setting_on("folders_first"), toggle_setting("folders_first")),
		common.separator(),
		ui.menu_item {
			"Pin to Quick access",
			on_click = function()
				ui.invoke("pin_current")
			end,
		},
		ui.menu_item {
			"Select all",
			on_click = function()
				ui.invoke("select_all")
			end,
		},
		ui.show(
			function()
				local s = state.storage:get()
				return s ~= nil and s.instant == true
			end,
			ui.menu_item {
				"Turn off instant scan…",
				on_click = function()
					ui.invoke("remove_instant_scan")
				end,
			}
		),
	}
end

return function(): Node
	return ui.row {
		w_full = true,
		px = 8,
		pt = 2,
		pb = 8,
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
			ui.div { w = 4 },
			ui.show(function()
				local n = state.nav:get()
				return not (n and n.overview)
			end, ui.row { gap = 4, items = "center", view_switch(), sort_menu() }),
			more_menu(),
		},
	}
end
