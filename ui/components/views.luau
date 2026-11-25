-- The folder's contents in three layouts: details (columns), list
-- (compact), and grid (big icons). All are virtual lists: only rows on
-- screen are built, so 100,000 items cost what 30 do.

local state = require("../state")
local theme = require("../theme")
local common = require("./common")

local motion = theme.motion(state.setting)

-- When the current folder's rows last changed: rows mounting soon after
-- fade in with a short stagger (scrolling later mounts rows without it).
local loaded_at = 0
ui.effect(function()
	state.rows:len()
	state.nav:get()
	loaded_at = ui.now()
end)

local function entrance(index: () -> number)
	return function(el)
		if not motion() or ui.now() - loaded_at > 350 then
			return
		end
		local i = math.min(index(), 24)
		anime.animate(el, {
			opacity = { 0, 1 },
			translateY = { 6, 0 },
			duration = 280,
			delay = (i - 1) * 12,
			ease = "out(3)",
		})
	end
end

local DETAILS_ROW = 30
local LIST_ROW = 26
local COL_DATE = 170
local COL_TYPE = 190
local COL_SIZE = 110

-- What a drag carries: the paths of the selection when the row is part of
-- it, else just the row.
type Drag = { paths: { string } }

local function drag_payload(row: state.Row): Drag
	if state.is_selected(row.id) then
		local paths = {}
		for id in state.sel:peek().ids do
			table.insert(paths, state.path_of(id))
		end
		return { paths = paths }
	end
	return { paths = { state.path_of(row.id) } }
end

local function drop_on(target: string?, paths: { string })
	ui.invoke("drop", { paths = paths, target = target, copy = ui.modifiers.control:peek() })
end

local function file_paths(files: { any }): { string }
	local out = {}
	for _, f in files do
		table.insert(out, f.path)
	end
	return out
end

local function base_name(path: string): string
	return string.match(path, "([^/\\]+)[/\\]*$") or path
end

-- What a press on an item does, shared by every layout.
local function item_handlers(index: () -> number, row: () -> state.Row)
	return {
		-- Drag out to other apps or onto folders; folders take drops and
		-- spring open when a drag rests on them.
		on_drag = function(): Drag?
			if state.renaming:peek() ~= nil then
				return nil
			end
			return drag_payload(row())
		end,
		drag_preview = function(payload: Drag)
			return ui.row {
				px = 10,
				py = 6,
				gap = 8,
				items = "center",
				rounded = "$radius.md",
				bg = "$surface.overlay",
				border = 1,
				border_color = "$focus.ring",
				shadow = { x = 0, y = 0, blur = 14, spread = 0, color = "$app.glow" },
				ui.text {
					if #payload.paths == 1 then base_name(payload.paths[1]) else `{#payload.paths} items`,
					literal = true,
					weight = 500,
				},
			}
		end,
		drag_preview_anchor = { 0, 0.5 },
		drag_preview_offset = { 14, 0 },
		drag_export = function(payload: Drag)
			return { files = payload.paths }
		end,
		drop_when = function(payload: any)
			local r = row()
			if not r.dir or type(payload) ~= "table" or payload.paths == nil then
				return false
			end
			local target = state.path_of(r.id)
			for _, p in payload.paths do
				if p == target then
					return false
				end
			end
			return true
		end,
		on_drop = function(payload: Drag)
			drop_on(state.path_of(row().id), payload.paths)
		end,
		on_drag_spring = function()
			if row().dir then
				ui.invoke("open", index())
				return true
			end
			return false
		end,
		on_file_drop = function(files)
			drop_on(if row().dir then state.path_of(row().id) else nil, file_paths(files))
		end,
		on_click = function(e)
			local i = index()
			ui.invoke("click", { index = i, ctrl = e.ctrl == true, shift = e.shift == true })
			if state.setting("single_click_open") and not e.ctrl and not e.shift then
				ui.invoke("open", i)
			end
		end,
		on_double_click = function()
			ui.invoke("open", index())
		end,
		on_aux_click = function(e)
			if e.button == "middle" and row().dir then
				ui.invoke("open_new_tab", index())
			end
		end,
		context_menu = function()
			ui.invoke("context_menu", index())
			-- Explorer's own menu was shown natively: no zgx menu.
			return nil
		end,
	}
end

local function with(base: { [any]: any }, extra: { [any]: any }): { [any]: any }
	for k, v in extra do
		base[k] = v
	end
	return base
end

-- The name, or a field while renaming it in place (F2, the menu's Rename,
-- a new folder). Enter or leaving the field renames; Escape cancels.
local function name_cell(r: () -> state.Row, index: () -> number, grow: boolean?): Node
	-- What is typed, followed with on_change, so Enter can commit it and
	-- stop there (it must not reach the window's Enter = open).
	local draft: string? = nil
	local function commit(t: string?)
		if state.renaming:peek() ~= nil then
			state.renaming:set(nil)
			-- The Enter that ended it must not also open the item.
			state.rename_ended_at = ui.now()
			if t ~= nil then
				ui.invoke("rename", { index = index(), name = t })
			end
		end
	end
	return ui.show(
		function()
			return state.renaming:get() == r().id
		end,
		ui.input {
			value = function()
				return r().id
			end,
			label = "New name",
			h = 24,
			px = 6,
			py = 0,
			grow = 1,
			min_w = 80,
			bg = "$surface.sunken",
			border = 1,
			border_color = "$accent",
			rounded = "$radius.sm",
			caret_color = "$accent",
			on_mount = function(el)
				draft = nil
				el:focus()
				-- Select the name without its extension, as Explorer does
				-- (after the field has its text, a moment later).
				local name = r().id
				local dot = if r().dir then nil else string.find(name, "%.[^.]*$")
				ui.after(16, function()
					if dot and dot > 1 then
						el:set_selection(0, dot - 1)
					else
						el:select_all()
					end
				end)
			end,
			on_change = function(t: string)
				draft = t
			end,
			on_commit = function(t: string)
				commit(t)
			end,
			on_blur = function()
				commit(draft)
			end,
			on_key = function(e)
				if e.key == "Escape" then
					draft = nil
					commit(nil)
					return true
				elseif e.key == "Enter" then
					commit(draft)
					return true
				end
				return false
			end,
		},
		ui.text {
			function()
				return r().name
			end,
			literal = true,
			truncate = true,
			min_w = 0,
			grow = if grow then 1 else 0,
		}
	)
end

-- Cut items fade until they are pasted.
local function cut_opacity(r: () -> state.Row): () -> number
	return function()
		return if state.is_cut(r().id) then 0.45 else 1
	end
end

local function selected_bg(row: () -> state.Row): () -> string
	return function()
		return if state.is_selected(row().id) then "$selection.item" else "#00000000"
	end
end

-- Details ------------------------------------------------------------------

local function header_cell(label: string, key: string, width: number?, align: string?): Node
	return ui.button {
		label = `Sort by {label}`,
		on_click = ui.action("sort", key),
		w = width,
		grow = if width then 0 else 1,
		shrink = if width then 0 else 1,
		min_w = 0,
		h = 28,
		px = 6,
		rounded = 4,
		bg = "#00000000",
		hover = { bg = "$hover" },
		justify = if align == "end" then "end" else "start",
		items = "center",
		gap = 4,
		ui.text {
			string.upper(label),
			font = "$font.mono",
			text_size = 10.5,
			letter_spacing = 1,
			color = "$text.tertiary",
			truncate = true,
		},
		ui.text {
			function()
				local n = state.nav:get()
				if not n or n.sort ~= key then
					return ""
				end
				return if n.descending then "▾" else "▴"
			end,
			color = "$accent",
			text_size = 10,
		},
	}
end

local function details_header(): Node
	return ui.row {
		h = 32,
		w_full = true,
		px = 10,
		gap = 6,
		items = "center",
		border_sides = { 0, 0, 1, 0 },
		border_color = "$border.subtle",
		header_cell("Name", "name"),
		header_cell("Date modified", "date", COL_DATE),
		header_cell("Type", "type", COL_TYPE),
		header_cell("Size", "size", COL_SIZE, "end"),
	}
end

local function details_row(_item: state.Row, index: State<number>, row: State<state.Row>): Node
	local function r(): state.Row
		return row:get()
	end
	local function field(name: string): () -> string
		return function()
			return (row:get() :: any)[name]
		end
	end
	return ui.row(with({
		role = "option",
		label = field("name"),
		selected = function()
			return state.is_selected(r().id)
		end,
		relative = true,
		w_full = true,
		h = DETAILS_ROW,
		px = 12,
		gap = 12,
		items = "center",
		rounded = "$radius.sm",
		bg = selected_bg(r),
		hover = { bg = "$hover" },
		on_mount = entrance(function()
			return index:peek()
		end),
		common.pip(function()
			return state.is_selected(r().id)
		end),
		ui.row {
			grow = 1,
			min_w = 0,
			gap = 8,
			items = "center",
			-- Cut items fade (on the content: the row's own opacity belongs
			-- to its entrance animation).
			opacity = cut_opacity(r),
			common.file_icon(field("icon"), function()
				return r().dir
			end, 18),
			name_cell(r, function()
				return index:peek()
			end),
		},
		ui.text {
			field("date"),
			literal = true,
			w = COL_DATE,
			shrink = 0,
			color = "$text.secondary",
			font = "$font.mono",
			text_size = 12,
			truncate = true,
		},
		ui.text { field("kind"), literal = true, w = COL_TYPE, shrink = 0, color = "$text.secondary", truncate = true },
		ui.text {
			field("size"),
			literal = true,
			w = COL_SIZE,
			shrink = 0,
			align = "end",
			color = "$text.secondary",
			font = "$font.mono",
			text_size = 12,
		},
	}, item_handlers(function()
		return index:peek()
	end, function()
		return row:peek()
	end)))
end

-- List ---------------------------------------------------------------------

local function list_row(_item: state.Row, index: State<number>, row: State<state.Row>): Node
	local function r(): state.Row
		return row:get()
	end
	return ui.row(with({
		role = "option",
		label = function()
			return r().name
		end,
		selected = function()
			return state.is_selected(r().id)
		end,
		relative = true,
		w_full = true,
		h = LIST_ROW,
		px = 12,
		gap = 8,
		items = "center",
		rounded = "$radius.sm",
		bg = selected_bg(r),
		hover = { bg = "$hover" },
		on_mount = entrance(function()
			return index:peek()
		end),
		common.pip(function()
			return state.is_selected(r().id)
		end),
		ui.row {
			grow = 1,
			min_w = 0,
			gap = 8,
			items = "center",
			opacity = cut_opacity(r),
			common.file_icon(function()
				return r().icon
			end, function()
				return r().dir
			end, 16),
			name_cell(r, function()
				return index:peek()
			end, true),
		},
		ui.text {
			function()
				return r().size
			end,
			literal = true,
			color = "$text.tertiary",
			font = "$font.mono",
			text_size = 11.5,
			shrink = 0,
		},
	}, item_handlers(function()
		return index:peek()
	end, function()
		return row:peek()
	end)))
end

-- Grid ---------------------------------------------------------------------

local function tile_size(): (number, number)
	local icon = state.setting("icon_size") or 64
	return icon + 48, icon + 52
end

local function grid_cell(i: number): Node
	local function r(): state.Row?
		return state.rows:at(i)
	end
	local icon = state.setting("icon_size") or 64
	local tw, th = tile_size()
	return ui.column(with({
		role = "option",
		label = function()
			local x = r()
			return if x then x.name else ""
		end,
		selected = function()
			local x = r()
			return x ~= nil and state.is_selected(x.id)
		end,
		w = tw,
		h = th,
		p = 6,
		gap = 6,
		items = "center",
		rounded = "$radius.lg",
		border = 1,
		border_color = function()
			local x = r()
			return if x and state.is_selected(x.id) then "$focus.ring" else "#00000000"
		end,
		shadow = function()
			local x = r()
			return if x and state.is_selected(x.id)
				then { x = 0, y = 0, blur = 16, spread = 0, color = "$app.glow" }
				else false
		end,
		bg = function()
			local x = r()
			return if x and state.is_selected(x.id) then "$selection.item" else "#00000000"
		end,
		hover = { bg = "$hover" },
		on_mount = entrance(function()
			return i
		end),
		ui.column {
			w_full = true,
			gap = 6,
			items = "center",
			opacity = function()
				local x = r()
				return if x and state.is_cut(x.id) then 0.45 else 1
			end,
			common.thumbnail(function()
				local x = r()
				return if x then x.thumb else ""
			end, function()
				local x = r()
				return if x then x.icon else ""
			end, function()
				local x = r()
				return x ~= nil and x.dir
			end, icon),
			ui.text {
				function()
					local x = r()
					return if x then x.name else ""
				end,
				literal = true,
				align = "center",
				wrap = true,
				lines = 2,
				truncate = true,
				w_full = true,
				text_size = "$font.size.sm",
			},
		},
	}, item_handlers(function()
		return i
	end, function()
		return r() :: state.Row
	end)))
end

local function grid_row(r: number): Node
	local cols = state.grid_cols:peek()
	local cells = {}
	for c = 1, cols do
		local i = (r - 1) * cols + c
		if i <= state.rows:len() then
			table.insert(cells, grid_cell(i))
		end
	end
	-- Thumbnails are made only for rows on screen: say which this row shows,
	-- and that it stopped showing them as it scrolls away or its items change.
	ui.effect(function()
		local names = {}
		for c = 1, cols do
			local x = state.rows:at((r - 1) * cols + c)
			if x and x.thumb ~= "" then
				table.insert(names, x.thumb)
			end
		end
		if #names > 0 then
			ui.invoke("thumbs_show", names)
			ui.on_cleanup(function()
				ui.invoke("thumbs_hide", names)
			end)
		end
	end)
	local _, th = tile_size()
	return ui.row { h = th + 4, w_full = true, px = 8, gap = 4, table.unpack(cells) }
end

-- The list -------------------------------------------------------------------

local function keyed(item: state.Row): string
	return item.id
end

local function page_rows(): number
	local el = state.list_ref:peek()
	local h = if el then el.height else 600
	local row_h = if state.view() == "grid" then select(2, tile_size()) + 4 else DETAILS_ROW
	return math.max(1, math.floor(h / row_h))
end

-- Arrows, Page Up/Down, Home/End and type-to-jump, handled in Rust so the
-- selection rules are one place (crates/pane-app/src/selection.rs).
local typed = ""
local typed_at = 0
local function on_list_key(e): boolean
	if e.editing or e.alt then
		return false
	end
	local key = e.key
	local grid = state.view() == "grid"
	if grid and (key == "ArrowLeft" or key == "ArrowRight") then
		ui.invoke("move", { key = if key == "ArrowLeft" then "ArrowUp" else "ArrowDown", shift = e.shift, page = 1 })
		return true
	end
	if key == "ArrowUp" or key == "ArrowDown" then
		local by = if grid then state.grid_cols:peek() else 1
		ui.invoke("move", { key = if key == "ArrowUp" then "PageUp" else "PageDown", shift = e.shift, page = by })
		return true
	end
	if key == "PageUp" or key == "PageDown" then
		local rows = page_rows()
		ui.invoke("move", { key = key, shift = e.shift, page = if grid then rows * state.grid_cols:peek() else rows })
		return true
	end
	if key == "Home" or key == "End" then
		ui.invoke("move", { key = key, shift = e.shift })
		return true
	end
	if not e.ctrl and not e.meta and #key == 1 and key ~= " " then
		local now = ui.now()
		typed = if now - typed_at < 750 then typed .. key else key
		typed_at = now
		ui.invoke("jump", typed)
		return true
	end
	return false
end

local function list_of(kind: string): Node
	if kind == "grid" then
		return ui.list {
			ref = state.list_ref,
			count = function()
				local cols = state.grid_cols:get()
				return math.ceil(state.rows:len() / math.max(1, cols))
			end,
			-- Keyed by the column count too: a resize that changes it builds
			-- the rows again with the new number of cells.
			key = function(i: number)
				return `{state.grid_cols:peek()}:{i}`
			end,
			row = function(_key: string, index: State<number>)
				return grid_row(index:peek())
			end,
			height = select(2, tile_size()) + 4,
			scrollbar = true,
			w_full = true,
			grow = 1,
			py = 6,
			on_resize = function(size)
				local tw = tile_size()
				state.grid_cols:set(math.max(1, math.floor((size.w - 16) / (tw + 4))))
			end,
		}
	end
	return ui.list {
		ref = state.list_ref,
		data = state.rows,
		key = keyed,
		row = if kind == "list" then list_row else details_row,
		height = if kind == "list" then LIST_ROW else DETAILS_ROW,
		scrollbar = true,
		w_full = true,
		grow = 1,
		px = 6,
		py = 4,
	}
end

local function empty_or_error(): Node
	return ui.column {
		absolute = true,
		top = 80,
		left = 0,
		right = 0,
		items = "center",
		gap = 6,
		pointer_events = "none",
		ui.text {
			function()
				local n = state.nav:get()
				if n and n.error then
					return n.error
				end
				if n and n.loading then
					return ""
				end
				return if state.rows:len() == 0 then "This folder is empty." else ""
			end,
			color = "$text.secondary",
			wrap = true,
			align = "center",
			max_w = 520,
			px = 24,
		},
	}
end

-- Keep the keyboard's row in view as it moves.
ui.effect(function()
	local focus = state.sel:get().focus or 0
	if focus < 1 then
		return
	end
	local el = state.list_ref:peek()
	if not el then
		return
	end
	local row = if state.view() == "grid" then math.ceil(focus / math.max(1, state.grid_cols:peek())) else focus
	el:scroll_to_row(row, { align = "nearest" })
end)

return function(): Node
	return ui.column {
		grow = 1,
		w_full = true,
		min_h = 0,
		relative = true,
		focusable = true,
		label = "Items",
		on_key = on_list_key,
		-- Right-click on empty space: the folder's own menu (New, Paste, ...).
		context_menu = function()
			ui.invoke("background_menu")
			return nil
		end,
		-- Files dropped on empty space land in this folder: from other apps,
		-- or dragged in Pane from another folder (a folder that sprang open).
		on_file_drop = function(files)
			drop_on(nil, file_paths(files))
		end,
		drop_when = function(payload: any)
			if type(payload) ~= "table" or payload.paths == nil or not state.nav:peek() then
				return false
			end
			local here = string.lower(state.nav:peek().path)
			for _, p in payload.paths do
				local parent = string.lower(string.match(p, "^(.*)[/\\][^/\\]+$") or "")
				if parent ~= here and parent .. "\\" ~= here then
					return true
				end
			end
			return false
		end,
		on_drop = function(payload: Drag)
			drop_on(nil, payload.paths)
		end,
		-- A click on empty space clears the selection.
		on_click = function(e)
			if not e.ctrl and not e.shift then
				ui.invoke("select_none")
			end
		end,
		ui.show(function()
			return state.view() == "details"
		end, details_header()),
		ui.switch(state.view, {
			details = list_of("details"),
			list = list_of("list"),
			grid = list_of("grid"),
		}),
		empty_or_error(),
	}
end
