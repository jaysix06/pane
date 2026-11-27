-- The Settings window (Ctrl+,, or More → Settings), in the Windows 11
-- Settings style: pages on the left with a search box that finds a setting
-- on any page, and each page's settings as rows of cards on the right, with
-- "Reset this page". Every change applies at once and is saved by Rust
-- (set_setting, crates/pane-app/src/settings.rs).

local state = require("./state")
local theme = require("./theme")
local icons = require("./components/icons")
local customize = require("./components/customize")
local shortcuts = require("./shortcuts")

local M = {}

M.open = ui.state("settings_open", false)
local page = ui.state("settings_page", "general")
local query = ui.state("settings_query", "")
local find_focused = ui.state("settings_find_focused", false)

function M.show(which: string?)
	if which then
		page:set(which)
	end
	query:set("")
	ui.invoke("about_refresh")
	M.open:set(true)
end
shortcuts.open_settings = M.show

local function set(key: string, value: any)
	ui.invoke("set_setting", { key = key, value = value })
end

-- Controls --------------------------------------------------------------------

local function switch(on: () -> boolean, flip: () -> (), label: string): Node
	return ui.button {
		role = "switch",
		label = label,
		checked = on,
		on_click = flip,
		w = 40,
		h = 22,
		p = 0,
		shrink = 0,
		rounded = "$radius.full",
		bg = function()
			return if on() then "$accent" else "$surface.raised"
		end,
		border = 1,
		border_color = function()
			return if on() then "$accent" else "$border.strong"
		end,
		shadow = function()
			return if on() then { x = 0, y = 0, blur = 10, spread = 0, color = "$app.glow" } else false
		end,
		relative = true,
		ui.div {
			absolute = true,
			top = 3,
			left = function()
				return if on() then 21 else 3
			end,
			w = 14,
			h = 14,
			rounded = "$radius.full",
			bg = function()
				return if on() then "$accent.text" else "$text.secondary"
			end,
		},
	}
end

local function setting_switch(key: string, label: string): Node
	return switch(function()
		return state.setting(key) == true
	end, function()
		set(key, not state.setting(key))
	end, label)
end

local function choice(key: string, label: string, options: { { label: string, value: any } }): Node
	return ui.select {
		label = label,
		options = options,
		value = function()
			return state.setting(key)
		end,
		on_change = function(v)
			set(key, v)
		end,
		w = 200,
		shrink = 0,
	}
end

local function button(label: string, on_click: () -> (), props: { [string]: any }?): Node
	local p: { [string]: any } = {
		label = label,
		on_click = on_click,
		px = 12,
		h = 32,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.raised",
		border = 1,
		border_color = "$border.subtle",
		hover = { border_color = "$border.strong" },
		shrink = 0,
		ui.text { label },
	}
	for k, v in props or {} do
		p[k] = v
	end
	return ui.button(p)
end

-- Rows ----------------------------------------------------------------------

-- A setting: what it's called, a line about it, its control, and the
-- settings keys "Reset this page" puts back.
export type Row = { label: string, hint: string?, keys: { string }?, control: () -> Node, wide: boolean? }

local function card(row: Row): Node
	if row.wide then
		return ui.column {
			w_full = true,
			p = 16,
			gap = 12,
			rounded = "$radius.md",
			bg = "$surface.base",
			border = 1,
			border_color = "$border.subtle",
			ui.column {
				gap = 2,
				ui.text { row.label, weight = 600 },
				ui.show(row.hint ~= nil, ui.text {
					row.hint or "",
					color = "$text.secondary",
					text_size = 12.5,
					wrap = true,
				}),
			},
			row.control(),
		}
	end
	return ui.row {
		w_full = true,
		min_h = 64,
		px = 16,
		py = 12,
		gap = 16,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.base",
		border = 1,
		border_color = "$border.subtle",
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 2,
			ui.text { row.label, wrap = true },
			ui.show(row.hint ~= nil, ui.text {
				row.hint or "",
				color = "$text.secondary",
				text_size = 12.5,
				wrap = true,
			}),
		},
		row.control(),
	}
end

local function toggle(key: string, label: string, hint: string?): Row
	return {
		label = label,
		hint = hint,
		keys = { key },
		control = function()
			return setting_switch(key, label)
		end,
	}
end

local function pick(key: string, label: string, hint: string?, options: { { label: string, value: any } }): Row
	return {
		label = label,
		hint = hint,
		keys = { key },
		control = function()
			return choice(key, label, options)
		end,
	}
end

-- Pages ---------------------------------------------------------------------

local function about(): any
	return state.about:get() or {}
end

local function start_folder_options(): { { label: string, value: any } }
	local options = {
		{ label = "Overview", value = "overview" },
		{ label = "Home folder", value = "home" },
		{ label = "This PC", value = "this_pc" },
		{ label = "Downloads", value = "downloads" },
	}
	local current = state.setting("start_folder")
	local known = false
	for _, o in options do
		known = known or o.value == current
	end
	if not known and type(current) == "string" then
		table.insert(options, { label = current, value = current })
	end
	return options
end

local function pinned_list(): Node
	local pins = ui.computed(function(): { string }
		return state.setting("pinned") or {}
	end)
	-- By path, not by the row's index: a row is kept as its pin moves, so
	-- the index it was built with goes stale after the first move.
	local function move(path: string, by: number)
		local list = table.clone(pins:peek())
		local i = table.find(list, path)
		local j = (i or 0) + by
		if not i or j < 1 or j > #list then
			return
		end
		list[i], list[j] = list[j], list[i]
		set("pinned", list)
	end
	return ui.column {
		w_full = true,
		gap = 4,
		ui.show(function()
			return #pins:get() == 0
		end, ui.text {
			"Nothing pinned yet. Right-click a folder, or use More → Pin to Quick access.",
			color = "$text.tertiary",
			text_size = 12.5,
			wrap = true,
		}),
		ui.each(pins, function(path: string)
			return ui.row {
				w_full = true,
				h = 36,
				px = 10,
				gap = 6,
				items = "center",
				rounded = "$radius.sm",
				bg = "$surface.sunken",
				ui.div { color = "$accent", icons.icon("folder", 14) },
				ui.text { path, literal = true, grow = 1, min_w = 0, truncate = true, font = "$font.mono", text_size = 12 },
				button("Up", function()
					move(path, -1)
				end, { h = 26, px = 8 }),
				button("Down", function()
					move(path, 1)
				end, { h = 26, px = 8 }),
				button("Unpin", function()
					ui.invoke("unpin", path)
				end, { h = 26, px = 8 }),
			}
		end),
	}
end

-- The keyboard shortcuts page: each command with its keys; "Change" waits
-- for the next key pressed, and a key another command has asks first.
local capturing = ui.state("shortcut_capturing", nil :: string?)
local conflict = ui.state("shortcut_conflict", nil :: { id: string, key: string, other: string }?)

local function shortcut_row(c: shortcuts.Command): Node
	local function keys_text(): string
		local keys = shortcuts.keys(c)
		if #keys == 0 then
			return "None"
		end
		local labels = {}
		for _, k in keys do
			table.insert(labels, shortcuts.label(k))
		end
		return table.concat(labels, "  ·  ")
	end
	return ui.column {
		w_full = true,
		gap = 6,
		ui.row {
			w_full = true,
			h = 40,
			px = 12,
			gap = 8,
			items = "center",
			rounded = "$radius.sm",
			bg = function()
				return if capturing:get() == c.id then "$selection.item" else "$surface.sunken"
			end,
			ui.text { c.label, grow = 1, min_w = 0, truncate = true },
			ui.text {
				function()
					return if capturing:get() == c.id then "Press a key…" else keys_text()
				end,
				font = "$font.mono",
				text_size = 12,
				color = function()
					return if shortcuts.is_custom(c) then "$accent" else "$text.secondary"
				end,
			},
			ui.switch(function()
				return if capturing:get() == c.id then "capture" else "idle"
			end, {
				capture = button("Cancel", function()
					capturing:set(nil)
				end, {
					h = 28,
					px = 10,
					focusable = true,
					-- The next key pressed (not a modifier alone) is the new key.
					on_mount = function(el)
						el:focus()
					end,
					on_key = function(e)
						local key = shortcuts.key_of(e)
						if not key then
							return true
						end
						if key == "escape" then
							capturing:set(nil)
							return true
						end
						local other = shortcuts.command_for(key)
						capturing:set(nil)
						if other and other.id ~= c.id then
							conflict:set({ id = c.id, key = key, other = other.label })
						else
							conflict:set(nil)
							shortcuts.set(c.id, key)
						end
						return true
					end,
				}),
				idle = ui.row {
					gap = 6,
					button("Change", function()
						conflict:set(nil)
						capturing:set(c.id)
					end, { h = 28, px = 10 }),
					button("Clear", function()
						shortcuts.set(c.id, "")
					end, { h = 28, px = 10 }),
					ui.show(function()
						return shortcuts.is_custom(c)
					end, button("Reset", function()
						shortcuts.set(c.id, nil)
					end, { h = 28, px = 10 })),
				},
			}),
		},
		ui.show(function()
			local x = conflict:get()
			return x ~= nil and x.id == c.id
		end, ui.row {
			w_full = true,
			px = 12,
			gap = 8,
			items = "center",
			ui.text {
				function()
					local x = conflict:get()
					return if x then `{shortcuts.label(x.key)} is used by {x.other}.` else ""
				end,
				color = "$warning",
				text_size = 12.5,
				grow = 1,
				wrap = true,
			},
			button("Use it here", function()
				local x = conflict:peek()
				conflict:set(nil)
				if x then
					shortcuts.set(x.id, x.key)
				end
			end, { h = 28, px = 10 }),
			button("Cancel", function()
				conflict:set(nil)
			end, { h = 28, px = 10 }),
		}),
	}
end

local reset_all_armed = ui.state("settings_reset_all", false)

export type Page = { id: string, label: string, icon: string, rows: { Row } }

local pages: { Page } = {
	{
		id = "general",
		label = "General",
		icon = "home",
		rows = {
			pick("start_folder", "Open Pane to", "Where a new window and a new tab start.", {}),
			toggle("restore_session", "Reopen your tabs", "Pane opens with the tabs you had open last time."),
			toggle(
				"open_folders_in_new_tab",
				"Open folders in a new tab",
				"Double-clicking a folder opens it in a tab of its own."
			),
			toggle("single_click_open", "Open items with one click", "Instead of double-clicking."),
			{
				label = "Open folders in Pane",
				hint = "Folders, drives and Win+E open in Pane instead of File Explorer. Turn it off to give them back to the app that had them.",
				control = function()
					return switch(function()
						return about().default_fm == true
					end, function()
						ui.invoke("set_default_fm", not about().default_fm)
					end, "Open folders in Pane")
				end,
			},
		},
	},
	{
		id = "appearance",
		label = "Appearance",
		icon = "palette",
		rows = {
			{
				label = "Look",
				hint = "Pane's colour, glow, theme, corners, density, motion and backdrop.",
				keys = { "accent", "glow", "theme", "corners", "density", "animations", "backdrop" },
				wide = true,
				control = customize.panel,
			},
			pick("default_view", "Layout for new tabs", nil, {
				{ label = "Details", value = "details" },
				{ label = "List", value = "list" },
				{ label = "Grid", value = "grid" },
			}),
			pick("icon_size", "Grid icon size", nil, {
				{ label = "Small", value = 48 },
				{ label = "Medium", value = 64 },
				{ label = "Large", value = 96 },
				{ label = "Extra large", value = 128 },
			}),
		},
	},
	{
		id = "files",
		label = "Files & folders",
		icon = "folder",
		rows = {
			toggle("show_hidden", "Show hidden files and folders"),
			toggle(
				"show_system",
				"Show protected operating system files",
				"Only with hidden files shown. Windows needs these; deleting them can stop it working."
			),
			toggle("show_extensions", "Show file name extensions", "report.pdf rather than report."),
			toggle("folders_first", "Folders first", "Folders above files, whatever the sort."),
			toggle("natural_sort", "Sort numbers by value", "file2 before file10, as people count."),
			toggle("relative_dates", "Friendly dates", "Today 9:26 AM and Mon 10:37 PM rather than 11/26/2026 9:26 AM."),
			toggle(
				"folder_sizes",
				"Show folder sizes",
				"Works out each folder's size in the background. Not on network drives."
			),
			toggle("confirm_delete", "Ask before deleting", "Moving to the Recycle Bin asks first."),
			toggle(
				"delete_permanently",
				"Delete permanently",
				"Skip the Recycle Bin. Deleted files can't be brought back."
			),
		},
	},
	{
		id = "sidebar",
		label = "Sidebar",
		icon = "pin",
		rows = {
			toggle("sidebar_quick_access", "Show pinned folders"),
			toggle("sidebar_known_folders", "Show Home, Desktop, Downloads and the other folders"),
			toggle("sidebar_drives", "Show drives"),
			{
				label = "Pinned folders",
				hint = "Their order in the sidebar.",
				keys = { "pinned" },
				wide = true,
				control = pinned_list,
			},
		},
	},
	{
		id = "thumbnails",
		label = "Thumbnails & preview",
		icon = "image",
		rows = {
			toggle("thumbnails", "Show thumbnails", "Pictures and videos show what's in them in grid view."),
			toggle(
				"thumbnails_network",
				"Thumbnails on network drives",
				"Reading files over the network for thumbnails can be slow."
			),
			toggle("preview_pane", "Show the preview pane", "Alt+P shows and hides it."),
			pick("preview_max_kb", "Text preview size", "How much of a text or code file the preview reads.", {
				{ label = "256 KB", value = 256 },
				{ label = "1 MB", value = 1024 },
				{ label = "4 MB", value = 4096 },
				{ label = "16 MB", value = 16384 },
			}),
		},
	},
	{
		id = "search",
		label = "Search",
		icon = "search",
		rows = {
			toggle("search_hidden", "Search hidden files and folders"),
			toggle("search_case_sensitive", "Match case", "Report and report are different."),
			toggle("search_whole_name", "Match whole names", "Otherwise any part of a name matches."),
			pick("search_max_depth", "How deep to search", "Folders below the one searched.", {
				{ label = "This folder only", value = 1 },
				{ label = "2 levels", value = 2 },
				{ label = "4 levels", value = 4 },
				{ label = "8 levels", value = 8 },
				{ label = "32 levels", value = 32 },
				{ label = "Everything", value = 256 },
			}),
		},
	},
	{
		id = "keys",
		label = "Keyboard shortcuts",
		icon = "command",
		rows = {
			{
				label = "Shortcuts",
				hint = `Click Change, then press the keys. Enter, Escape and Ctrl+1 to Ctrl+9 stay as they are.`,
				keys = { "shortcuts" },
				wide = true,
				control = function()
					local rows = {}
					for _, c in shortcuts.list do
						table.insert(rows, shortcut_row(c))
					end
					return ui.column { w_full = true, gap = 4, table.unpack(rows) }
				end,
			},
		},
	},
	{
		id = "advanced",
		label = "Advanced",
		icon = "chip",
		rows = {
			toggle("check_updates", "Check for updates automatically", "Every few hours, from Pane's GitHub releases."),
			pick("thumbnail_threads", "Thumbnail threads", "How many thumbnails are made at once. From the next start.", {
				{ label = "1", value = 1 },
				{ label = "2", value = 2 },
				{ label = "3", value = 3 },
				{ label = "4", value = 4 },
				{ label = "6", value = 6 },
				{ label = "8", value = 8 },
			}),
			{
				label = "Clear caches",
				hint = "Thumbnails are made again as they show.",
				control = function()
					return button("Clear", function()
						ui.invoke("clear_caches")
					end)
				end,
			},
			{
				label = "Settings folder",
				hint = "Where settings.json and session.json are kept.",
				control = function()
					return button("Open", function()
						ui.invoke("open_settings_folder")
					end)
				end,
			},
			{
				label = "Export or import settings",
				hint = "A file to keep, or to bring your settings to another PC.",
				control = function()
					return ui.row {
						gap = 6,
						button("Export…", function()
							ui.task(function()
								local file = ui.save_file({ name = "pane-settings.json" }):await()
								if file then
									ui.invoke("export_settings", file.path)
								end
							end)
						end),
						button("Import…", function()
							ui.task(function()
								local files = ui.open_files({
									title = "Import Pane settings",
									filters = { { "Pane settings", { "json" } } },
								}):await()
								if files and files[1] then
									ui.invoke("import_settings", files[1].path)
								end
							end)
						end),
					}
				end,
			},
			{
				label = "Reset everything",
				hint = "Every setting back to how Pane came. Pinned folders are unpinned.",
				control = function()
					return ui.switch(function()
						return if reset_all_armed:get() then "armed" else "idle"
					end, {
						idle = button("Reset…", function()
							reset_all_armed:set(true)
						end),
						armed = ui.row {
							gap = 6,
							button("Cancel", function()
								reset_all_armed:set(false)
							end),
							button("Reset everything", function()
								reset_all_armed:set(false)
								ui.invoke("reset_settings", { keys = {} })
							end, { bg = "$danger", border_color = "$danger", color = "#FFFFFF" }),
						},
					})
				end,
			},
		},
	},
}

-- "Open Pane to" lists a custom start folder too, so it's built as it shows.
pages[1].rows[1].control = function()
	return ui.select {
		label = "Open Pane to",
		options = ui.computed(start_folder_options),
		value = function()
			return state.setting("start_folder")
		end,
		on_change = function(v)
			set("start_folder", v)
		end,
		w = 200,
		shrink = 0,
	}
end

local function page_of(id: string): Page
	for _, p in pages do
		if p.id == id then
			return p
		end
	end
	return pages[1]
end

local function page_keys(p: Page): { string }
	local keys = {}
	for _, row in p.rows do
		for _, k in row.keys or {} do
			table.insert(keys, k)
		end
	end
	return keys
end

-- Search: rows on any page whose name or line says what was typed, as
-- { page, row } indexes (state holds no functions, and rows build theirs).
local function matches(): { { page: number, row: number } }
	local q = string.lower(query:get())
	local out = {}
	if q == "" then
		return out
	end
	for pi, p in pages do
		for ri, row in p.rows do
			local text = string.lower(row.label .. " " .. (row.hint or "") .. " " .. p.label)
			if string.find(text, q, 1, true) then
				table.insert(out, { page = pi, row = ri })
			end
		end
	end
	return out
end

-- Layout ----------------------------------------------------------------------

local function nav(): Node
	local items = {}
	for _, p in pages do
		table.insert(items, ui.button {
			label = p.label,
			on_click = function()
				query:set("")
				page:set(p.id)
			end,
			w_full = true,
			h = 38,
			px = 12,
			gap = 12,
			items = "center",
			rounded = "$radius.md",
			bg = function()
				return if page:get() == p.id and query:get() == "" then "$surface.raised" else "#00000000"
			end,
			color = function()
				return if page:get() == p.id and query:get() == "" then "$text.primary" else "$text.secondary"
			end,
			hover = { bg = "$hover", color = "$text.primary" },
			relative = true,
			ui.show(function()
				return page:get() == p.id and query:get() == ""
			end, ui.div {
				absolute = true,
				left = 0,
				top = 11,
				w = 3,
				h = 16,
				rounded = "$radius.full",
				bg = "$accent",
				shadow = { x = 0, y = 0, blur = 8, spread = 0, color = "$app.glow" },
			}),
			ui.div { color = "$accent", icons.icon(p.icon, 16) },
			ui.text { p.label },
		})
	end
	return ui.column {
		w = 260,
		shrink = 0,
		h_full = true,
		p = 12,
		gap = 4,
		ui.row {
			w_full = true,
			gap = 10,
			items = "center",
			px = 6,
			pb = 8,
			ui.image { src = "images/pane.png", w = 28, h = 28, label = "Pane" },
			ui.text { "Settings", weight = 600, text_size = 18 },
		},
		ui.row {
			w_full = true,
			h = 34,
			px = 10,
			gap = 8,
			mb = 8,
			items = "center",
			rounded = "$radius.md",
			bg = "$surface.sunken",
			border = 1,
			border_color = function()
				return if find_focused:get() then "$accent" else "$border.subtle"
			end,
			color = "$text.tertiary",
			icons.icon("search", 14),
			ui.input {
				value = query,
				label = "Find a setting",
				placeholder = "Find a setting",
				grow = 1,
				min_w = 0,
				h = 30,
				px = 0,
				bg = "#00000000",
				text_size = 13,
				caret_color = "$accent",
				-- The box around it draws the border, amber while focused.
				border = 0,
				focus = { border = 0 },
				on_focus = function()
					find_focused:set(true)
				end,
				on_blur = function()
					find_focused:set(false)
				end,
			},
		},
		table.unpack(items),
	}
end

local function page_view(): Node
	return ui.column {
		w_full = true,
		gap = 6,
		ui.row {
			w_full = true,
			items = "center",
			pb = 12,
			ui.text {
				function()
					return page_of(page:get()).label
				end,
				weight = 600,
				text_size = 26,
				grow = 1,
			},
			ui.show(function()
				return #page_keys(page_of(page:get())) > 0
			end, button("Reset this page", function()
				ui.invoke("reset_settings", { keys = page_keys(page_of(page:get())) })
			end)),
		},
		ui.switch(page, (function()
			local views = {}
			for _, p in pages do
				local cards = {}
				for _, row in p.rows do
					table.insert(cards, card(row))
				end
				views[p.id] = ui.column { w_full = true, gap = 6, table.unpack(cards) }
			end
			return views
		end)()),
	}
end

local function results_view(): Node
	local found = ui.computed(matches)
	return ui.column {
		w_full = true,
		gap = 6,
		ui.text {
			function()
				local n = #found:get()
				return if n == 0 then `Nothing matches “{query:get()}”` else `Results for “{query:get()}”`
			end,
			weight = 600,
			text_size = 26,
			pb = 12,
		},
		ui.each(found, function(at: any)
			local hit = { page = pages[at.page], row = pages[at.page].rows[at.row] }
			return ui.column {
				w_full = true,
				gap = 4,
				ui.button {
					label = `Open {hit.page.label}`,
					on_click = function()
						query:set("")
						page:set(hit.page.id)
					end,
					bg = "#00000000",
					p = 0,
					color = "$text.tertiary",
					hover = { color = "$accent" },
					ui.text { string.upper(hit.page.label), font = "$font.mono", text_size = 10.5, letter_spacing = 1 },
				},
				card(hit.row),
			}
		end),
	}
end

local function body(): Node
	return ui.row {
		w_full = true,
		h_full = true,
		theme = theme.scope(state.setting),
		bg = "$app.chrome",
		nav(),
		ui.scroll {
			grow = 1,
			min_w = 0,
			h_full = true,
			bg = "$surface.base",
			rounded_tl = "$radius.lg",
			border_sides = { 1, 0, 0, 1 },
			border_color = "$border.subtle",
			scrollbar = true,
			ui.column {
				w_full = true,
				max_w = 860,
				px = 32,
				py = 28,
				ui.show(function()
					return query:get() ~= ""
				end, results_view(), page_view()),
			},
		},
	}
end

ui.window {
	id = "settings",
	title = "Pane Settings",
	width = 1000,
	height = 720,
	frame_autosave = "settings",
	open = M.open,
	body(),
}

return M
