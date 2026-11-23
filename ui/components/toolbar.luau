-- The toolbar under the tabs: back / forward / up / refresh, the address
-- bar (breadcrumbs, or a text field while editing), and the View, Sort and
-- More menus.

local state = require("../state")
local common = require("./common")

local function nav_flag(name: string): () -> boolean
	return function()
		local n = state.nav:get()
		return n ~= nil and (n :: any)[name] == true
	end
end

local function start_editing()
	local n = state.nav:peek()
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
		gap = 0,
		items = "center",
		-- The end of the path is what matters: overflow is cut at the start.
		justify = "end",
		overflow = "hidden",
		ui.row {
			shrink = 0,
			items = "center",
			ui.each(crumbs, function(crumb: state.Crumb, index: number)
				return ui.row {
					items = "center",
					shrink = 0,
					ui.show(index > 1, ui.text { "›", color = "$text.tertiary", px = 1 }),
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
						rounded = 4,
						bg = "#00000000",
						hover = { bg = "$hover" },
						ui.text { crumb.label, literal = true },
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

local function address_bar(): Node
	return ui.row {
		grow = 1,
		min_w = 0,
		h = 34,
		px = 4,
		items = "center",
		rounded = 6,
		bg = "$surface.raised",
		border = 1,
		border_color = function()
			return if state.editing_address:get() then "$accent" else "$border.subtle"
		end,
		ui.switch(function()
			return if state.editing_address:get() then "edit" else "crumbs"
		end, {
			edit = address_input(),
			crumbs = crumbs_view(),
		}),
	}
end

local function view_menu(): Node
	local open = ui.state("view_menu_open", false)
	local function view_is(v: string): () -> boolean
		return function()
			return state.view() == v
		end
	end
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
		label = "View",
		anchor = ui.button {
			on_click = ui.toggle(open),
			tooltip = "Layout and what to show",
			px = 10,
			h = 32,
			rounded = 6,
			bg = "#00000000",
			hover = { bg = "$hover" },
			ui.text { "View ▾" },
		},
		common.check_item("Details", view_is("details"), function()
			ui.invoke("set_view", "details")
		end),
		common.check_item("List", view_is("list"), function()
			ui.invoke("set_view", "list")
		end),
		common.check_item("Grid", view_is("grid"), function()
			ui.invoke("set_view", "grid")
		end),
		common.separator(),
		common.check_item("Hidden items", setting_on("show_hidden"), toggle_setting("show_hidden")),
		common.check_item("File name extensions", setting_on("show_extensions"), toggle_setting("show_extensions")),
		common.check_item("Folders first", setting_on("folders_first"), toggle_setting("folders_first")),
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
		anchor = ui.button {
			on_click = ui.toggle(open),
			tooltip = "Sort order",
			px = 10,
			h = 32,
			rounded = 6,
			bg = "#00000000",
			hover = { bg = "$hover" },
			ui.text { "Sort ▾" },
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
	return ui.menu {
		open = open,
		label = "More",
		anchor = common.icon_button { label = "More options", glyph = "⋯", on_click = ui.toggle(open) },
		ui.menu_item {
			"Pin to Quick access",
			on_click = function()
				ui.invoke("pin_current")
			end,
		},
		ui.menu_item {
			"Open new tab",
			on_click = function()
				ui.invoke("new_tab")
			end,
		},
		common.separator(),
		ui.menu_item {
			"Select all",
			on_click = function()
				ui.invoke("select_all")
			end,
		},
		ui.menu_item {
			"Select none",
			on_click = function()
				ui.invoke("select_none")
			end,
		},
	}
end

return function(): Node
	return ui.row {
		h = 48,
		w_full = true,
		px = 8,
		gap = 4,
		items = "center",
		common.icon_button { label = "Back (Alt+Left)", glyph = "←", on_click = ui.action("back"), enabled = nav_flag("can_back") },
		common.icon_button {
			label = "Forward (Alt+Right)",
			glyph = "→",
			on_click = ui.action("forward"),
			enabled = nav_flag("can_forward"),
		},
		common.icon_button { label = "Up (Alt+Up)", glyph = "↑", on_click = ui.action("up"), enabled = nav_flag("can_up") },
		common.icon_button { label = "Refresh (F5)", glyph = "⟳", on_click = ui.action("refresh") },
		ui.div { w = 4 },
		address_bar(),
		ui.div { w = 4 },
		view_menu(),
		sort_menu(),
		more_menu(),
	}
end
