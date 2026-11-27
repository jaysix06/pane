-- The folder's contents in three layouts: details (columns), list
-- (compact), and grid (big icons). All are virtual lists: only rows on
-- screen are built, so 100,000 items cost what 30 do.

local state = require("../state")
local theme = require("../theme")
local common = require("./common")
local commands = require("./commands")

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
-- Date modified, Kind and Size have widths the header's handles drag
-- (saved as settings); Name takes the rest.
local COL_SETTING = { date = "col_date", type = "col_kind", size = "col_size" }
local COL_DEFAULT = { date = 170, type = 190, size = 140 }
local COL_MIN, COL_MAX = 60, 480
-- The column being dragged and its width, until the setting is saved.
local col_drag = ui.state("col_drag", nil :: { key: string, w: number }?)

local function col_w(key: string): number
	local d = col_drag:get()
	if d and d.key == key then
		return d.w
	end
	return state.setting(COL_SETTING[key]) or COL_DEFAULT[key]
end

local function col(key: string): () -> number
	return function()
		return col_w(key)
	end
end
-- A group header ("FOLDERS 8 ── 5.0 GB") above the first folder and the
-- first file, in details view.
local GROUP_H = 30
-- Grouping lays out every row's height; past this many it's skipped.
local GROUP_MAX = 20000
-- Name keeps at least this much: narrower, Type goes, then Date modified
-- (the preview pane open in a small window). Size always stays.
local NAME_MIN = 220

local function column_hidden(column: string): () -> boolean
	return function()
		local w = state.view_w:get()
		local with_date = NAME_MIN + col_w("date") + col_w("size") + 60
		return if column == "type" then w < with_date + col_w("type") + 12 else w < with_date
	end
end

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

-- A right-click on an item opens Explorer's menu natively, so zgx sees no
-- menu there and passes the click on to the folder view, which would open
-- the folder's own menu as well. The item claims the click; the view skips it.
local item_menu_claimed = false

-- A grid tile's width and height.
local function tile_size(): (number, number)
	local icon = state.setting("icon_size") or 64
	return icon + 48, icon + 52
end


-- Drag-select ---------------------------------------------------------------
-- A press on empty space (in details view, anywhere on a row but its icon
-- and name, as in Explorer) and a drag draws a rectangle; the items it
-- touches are selected. Ctrl or Shift keeps what was selected before. The
-- list scrolls while the pointer is past its top or bottom edge.

-- The list's scroll position, in points.
local list_offset = ui.state("list_offset", 0)
-- The folder view's column (the rectangle is drawn in it).
local view_ref = ui.ref()
-- The rectangle in list content points (x from the list's left, y from
-- the top of its content), and where the list sits in the view column.
type Rect = { x0: number, y0: number, x1: number, y1: number, top: number, left: number }
local marquee = ui.state("marquee", nil :: Rect?)

type Gesture = {
	x0: number,
	y0: number,
	-- The pointer, in window points.
	px: number,
	py: number,
	keep: boolean,
	moved: boolean,
	sent: string,
	stop_scroll: (() -> ())?,
}
local gesture: Gesture? = nil
-- An item's icon or name took this press: the list doesn't start a rectangle.
local item_pressed = false
-- A drag-select just ended: the click its release makes changes nothing.
local swallow_click = false

local function claim_press()
	item_pressed = true
end

-- Whether a click should be ignored (it ended a drag-select).
local function take_swallowed(): boolean
	if swallow_click then
		swallow_click = false
		return true
	end
	return false
end

local function dragging_select(): boolean
	return gesture ~= nil
end

local LIST_PY = 4
local GRID_PY = 6
local GRID_PX = 8

local function list_rect(): { x: number, y: number, width: number, height: number }?
	local el = state.list_ref:peek()
	return if el then ui.window_rect(el) else nil
end

-- Details and list: where row `i` starts in the content (group headers
-- make two rows taller), and the row at content point `y`.
local function row_top(i: number, row_h: number, starts: { number }): number
	local top = LIST_PY + (i - 1) * row_h
	for _, s in starts do
		if s < i then
			top += GROUP_H
		end
	end
	return top
end

local function group_starts(): { number }
	local out = {}
	if state.view() ~= "details" then
		return out
	end
	local g = state.groups:peek()
	local n = state.rows:len()
	if g == nil or n > GROUP_MAX then
		return out
	end
	if g.folders > 0 then
		table.insert(out, 1)
	end
	if g.files > 0 and g.folders + 1 <= n then
		table.insert(out, g.folders + 1)
	end
	return out
end

local function row_at(y: number, n: number, row_h: number, starts: { number }): number
	local i = math.clamp(math.floor((y - LIST_PY) / row_h) + 1, 1, n)
	while i > 1 and row_top(i, row_h, starts) > y do
		i -= 1
	end
	while i < n and row_top(i + 1, row_h, starts) <= y do
		i += 1
	end
	return i
end

-- The rows (from 1) the rectangle covers, as inclusive ranges.
local function covered(r: Rect): { { number } }
	local n = state.rows:len()
	if n == 0 then
		return {}
	end
	local xa, xb = math.min(r.x0, r.x1), math.max(r.x0, r.x1)
	local ya, yb = math.min(r.y0, r.y1), math.max(r.y0, r.y1)
	if state.view() == "grid" then
		local tw, th = tile_size()
		local cols = math.max(1, state.grid_cols:peek())
		local rows = math.ceil(n / cols)
		local pitch_y, pitch_x = th + 4, tw + 4
		-- Rows and columns whose tiles the rectangle overlaps (the gaps
		-- between tiles touch nothing).
		local ra = math.max(1, math.floor((ya - GRID_PY) / pitch_y) + 1)
		if ya - GRID_PY - (ra - 1) * pitch_y > th then
			ra += 1
		end
		local rb = math.min(rows, math.floor((yb - GRID_PY) / pitch_y) + 1)
		local ca = math.max(1, math.floor((xa - GRID_PX) / pitch_x) + 1)
		if xa - GRID_PX - (ca - 1) * pitch_x > tw then
			ca += 1
		end
		local cb = math.min(cols, math.floor((xb - GRID_PX) / pitch_x) + 1)
		if yb < GRID_PY or xb < GRID_PX or ra > rb or ca > cb then
			return {}
		end
		local out = {}
		for row = ra, rb do
			local a = (row - 1) * cols + ca
			local b = math.min(n, (row - 1) * cols + cb)
			if a <= b then
				table.insert(out, { a, b })
			end
		end
		return out
	end
	local row_h = if state.view() == "list" then LIST_ROW else DETAILS_ROW
	local starts = group_starts()
	local bottom = row_top(n + 1, row_h, starts)
	if yb < LIST_PY or ya >= bottom then
		return {}
	end
	return {
		{ row_at(math.max(ya, LIST_PY), n, row_h, starts), row_at(math.min(yb, bottom - 1), n, row_h, starts) },
	}
end

local function key_of(ranges: { { number } }): string
	local parts = {}
	for _, r in ranges do
		table.insert(parts, `{r[1]}-{r[2]}`)
	end
	return table.concat(parts, ",")
end

-- The rectangle's far corner follows the pointer; the selection follows
-- the rectangle.
local function update(g: Gesture)
	local rect = list_rect()
	local m = marquee:peek()
	if not rect or m == nil then
		return
	end
	local x1 = math.clamp(g.px - rect.x, 0, rect.width)
	local y1 = math.clamp(g.py - rect.y, 0, rect.height) + list_offset:peek()
	local r = { x0 = m.x0, y0 = m.y0, x1 = x1, y1 = y1, top = m.top, left = m.left }
	marquee:set(r)
	local ranges = covered(r)
	local k = key_of(ranges)
	if k ~= g.sent then
		g.sent = k
		ui.invoke("marquee", { ranges = ranges })
	end
end

local function finish(_e: any)
	local g = gesture
	gesture = nil
	state.root_pointer.move = nil
	state.root_pointer.release = nil
	if not g then
		return
	end
	if g.stop_scroll then
		g.stop_scroll()
	end
	if g.moved then
		marquee:set(nil)
		ui.invoke("marquee_end")
		-- Only the click this release makes, not a later one.
		ui.after(50, function()
			swallow_click = false
		end)
	end
end

local function follow(e: any)
	local g = gesture
	if not g then
		return
	end
	g.px, g.py = e.x, e.y
	if not g.moved then
		local rect = list_rect()
		if not rect then
			return
		end
		local dx = (e.x - rect.x) - g.x0
		local dy = (e.y - rect.y + list_offset:peek()) - g.y0
		if math.abs(dx) < 4 and math.abs(dy) < 4 then
			return
		end
		g.moved = true
		-- Whatever click the release makes, before or after this hears it.
		swallow_click = true
		local view = view_ref:peek()
		local vr = if view then ui.window_rect(view) else nil
		marquee:set {
			x0 = g.x0,
			y0 = g.y0,
			x1 = g.x0,
			y1 = g.y0,
			top = if vr then rect.y - vr.y else 0,
			left = if vr then rect.x - vr.x else 0,
		}
		ui.invoke("marquee_begin", { keep = g.keep })
		-- Past the top or bottom edge, the list scrolls, faster the further.
		g.stop_scroll = ui.every(16, function()
			local cur = gesture
			local r = list_rect()
			if not cur or not r then
				return
			end
			local over = 0
			if cur.py < r.y then
				over = cur.py - r.y
			elseif cur.py > r.y + r.height then
				over = cur.py - (r.y + r.height)
			end
			if over ~= 0 then
				list_offset:set(math.max(0, list_offset:peek() + math.clamp(over / 2, -40, 40)))
				update(cur)
			end
		end)
	end
	update(g)
end

-- The list's press: starts a rectangle unless an item took the press.
local function list_press(e: any)
	if item_pressed then
		item_pressed = false
		return
	end
	swallow_click = false
	if state.renaming:peek() ~= nil then
		return
	end
	local rect = list_rect()
	if not rect then
		return
	end
	gesture = {
		x0 = e.x - rect.x,
		y0 = e.y - rect.y + list_offset:peek(),
		px = e.x,
		py = e.y,
		keep = e.ctrl == true or e.shift == true,
		moved = false,
		sent = "",
		stop_scroll = nil,
	}
	state.root_pointer.move = follow
	state.root_pointer.release = finish
end

local function marquee_box(): Node
	local function m(): Rect
		return marquee:get() or { x0 = 0, y0 = 0, x1 = 0, y1 = 0, top = 0, left = 0 }
	end
	return ui.show(
		function()
			return marquee:get() ~= nil
		end,
		ui.div {
			absolute = true,
			top = function()
				return m().top
			end,
			left = function()
				return m().left
			end,
			right = 0,
			bottom = 0,
			overflow = "hidden",
			pointer_events = "none",
			ui.div {
				absolute = true,
				left = function()
					return math.min(m().x0, m().x1)
				end,
				top = function()
					return math.min(m().y0, m().y1) - list_offset:get()
				end,
				w = function()
					return math.abs(m().x1 - m().x0)
				end,
				h = function()
					return math.abs(m().y1 - m().y0)
				end,
				bg = "$selection.item",
				border = 1,
				border_color = "$accent",
				rounded = 2,
			},
		}
	)
end

-- What a press on an item does, shared by every layout.
-- `whole`: a press anywhere on the item is on it (list and grid); in
-- details view only its icon and name are, the rest starts a drag-select.
local function item_handlers(index: () -> number, row: () -> state.Row, whole: boolean)
	return {
		on_press = if whole then claim_press else nil,
		-- Drag out to other apps or onto folders; folders take drops and
		-- spring open when a drag rests on them.
		on_drag = function(): Drag?
			if state.renaming:peek() ~= nil or dragging_select() or state.in_recycle() then
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
			if take_swallowed() then
				return
			end
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
			item_menu_claimed = true
			ui.invoke("context_menu", index())
			-- The Recycle Bin's items: Pane's own menu (Rust selected it).
			if state.in_recycle() then
				return {
					ui.menu_item {
						"Restore",
						on_click = function()
							ui.invoke("restore")
						end,
					},
					ui.menu_item {
						"Delete permanently",
						on_click = function()
							commands.purge()
						end,
					},
				}
			end
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

-- Item checkboxes (Files & folders setting): a click on the box adds the
-- item to the selection or takes it out, as Ctrl+click does.
local function checkboxes_on(): boolean
	return state.setting("item_checkboxes") == true
end

local function check_box(checked: () -> boolean, on_toggle: () -> (), label: string, in_list: boolean): Node
	return ui.row {
		role = "checkbox",
		label = label,
		checked = checked,
		hidden = function()
			return not checkboxes_on()
		end,
		w = 16,
		h = 16,
		shrink = 0,
		items = "center",
		justify = "center",
		rounded = 4,
		border = 1,
		border_color = function()
			return if checked() then "$accent" else "$border.strong"
		end,
		bg = function()
			return if checked() then "$accent" else "$surface.sunken"
		end,
		-- In the list, a press on the box is on the item (no drag-select).
		on_press = if in_list then claim_press else nil,
		on_click = function()
			on_toggle()
		end,
		ui.text {
			function()
				return if checked() then "✓" else ""
			end,
			text_size = 11,
			weight = 700,
			color = "$accent.text",
		},
	}
end

local function item_check(index: () -> number, row: () -> state.Row?): Node
	return check_box(function()
		local r = row()
		return r ~= nil and state.is_selected(r.id)
	end, function()
		ui.invoke("click", { index = index(), ctrl = true, shift = false })
	end, "Select", true)
end

-- Details ------------------------------------------------------------------

-- The edge left of a sized column: dragging it left widens the column,
-- a double click puts it back to its usual width.
local function resize_handle(key: string): Node
	local start: { x: number, w: number }? = nil
	local function save(w: number)
		ui.invoke("set_setting", { key = COL_SETTING[key], value = math.floor(w + 0.5) })
		-- The drag's width shows until the saved setting comes back.
		ui.after(150, function()
			col_drag:set(nil)
		end)
	end
	return ui.div {
		absolute = true,
		left = -9,
		top = 4,
		bottom = 4,
		w = 9,
		cursor = "col_resize",
		label = "Resize column",
		items = "center",
		justify = "center",
		on_press = function(e)
			start = { x = e.x, w = col_w(key) }
		end,
		on_pointer_move = function(e)
			local st = start
			if st then
				col_drag:set { key = key, w = math.clamp(st.w - (e.x - st.x), COL_MIN, COL_MAX) }
			end
		end,
		on_release = function()
			local d = col_drag:peek()
			start = nil
			if d and d.key == key then
				save(d.w)
			end
		end,
		on_double_click = function()
			col_drag:set { key = key, w = COL_DEFAULT[key] }
			save(COL_DEFAULT[key])
		end,
		-- The line takes no pointer: a press lands on the handle itself,
		-- which then keeps the pointer until release.
		ui.div {
			w = 1,
			h_full = true,
			bg = "$border.subtle",
			pointer_events = "none",
		},
	}
end

local function header_cell(label: string | () -> string, key: string, sized: boolean?, align: string?): Node
	local button = ui.button {
		label = function()
			local text = if type(label) == "function" then label() else label
			return `Sort by {text}`
		end,
		on_click = ui.action("sort", key),
		w_full = sized == true,
		grow = if sized then 0 else 1,
		shrink = if sized then 0 else 1,
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
			function()
				return string.upper(if type(label) == "function" then label() else label)
			end,
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
	if not sized then
		return button
	end
	return ui.row {
		relative = true,
		w = col(key),
		shrink = 0,
		hidden = if key == "date" or key == "type" then column_hidden(key) else false,
		button,
		resize_handle(key),
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
		check_box(function()
			local n = state.rows:len()
			return n > 0 and state.sel:get().count == n
		end, function()
			local n = state.rows:len()
			ui.invoke(if n > 0 and state.sel:peek().count == n then "select_none" else "select_all")
		end, "Select all", false),
		header_cell("Name", "name"),
		header_cell(function()
			return if state.in_recycle() then "Date deleted" else "Date modified"
		end, "date", true),
		-- Search results show where each one is instead.
		header_cell(function()
			local n = state.nav:get()
			return if n and n.search then "Folder" elseif n and n.recycle then "Original location" else "Kind"
		end, "type", true),
		header_cell("Size", "size", true, "end"),
	}
end

-- Ten LEDs: a folder's part of everything in the folder on screen.
local function share_leds(share: () -> number): Node
	local cells = {}
	for k = 1, 10 do
		table.insert(
			cells,
			ui.div {
				w = 5,
				h = 9,
				rounded = 1.5,
				bg = function()
					local v = share()
					-- To the nearest tenth: a folder under 5% lights none.
					local lit = math.floor(v * 10 + 0.5)
					return if k <= lit then "$accent" else "$app.led_off"
				end,
			}
		)
	end
	return ui.row {
		gap = 2,
		shrink = 0,
		hidden = function()
			return share() < 0
		end,
		table.unpack(cells),
	}
end

-- Which group row `i` starts: "folders", "files", or nil.
local function group_at(i: number): string?
	local g = state.groups:get()
	if g == nil or state.view() ~= "details" or state.rows:len() > GROUP_MAX then
		return nil
	end
	if i == 1 and g.folders > 0 then
		return "folders"
	end
	if i == g.folders + 1 and g.files > 0 then
		return "files"
	end
	return nil
end

-- Each row's height: taller where a group starts.
local function row_heights(): { number }
	local n = state.rows:len()
	local out = table.create(n, DETAILS_ROW)
	local g = state.groups:get()
	if g ~= nil and n <= GROUP_MAX then
		if g.folders > 0 and n >= 1 then
			out[1] += GROUP_H
		end
		if g.files > 0 and g.folders + 1 <= n then
			out[g.folders + 1] += GROUP_H
		end
	end
	return out
end

-- "FOLDERS 8 ──── 5.0 GB" or "FILES 6 ──── 147 KB".
local function group_header(which: () -> string?): Node
	local function part(k: number): () -> string
		return function()
			local g = state.groups:get()
			if g == nil then
				return ""
			end
			if which() == "folders" then
				if k == 1 then
					return "FOLDERS"
				elseif k == 2 then
					return tostring(g.folders)
				end
				local s = state.sizes:get()
				local n = state.nav:get()
				if s == nil or n == nil or s.folder ~= n.path then
					return ""
				end
				return if s.done then s.folders_total else "Measuring…"
			end
			if k == 1 then
				return "FILES"
			elseif k == 2 then
				return tostring(g.files)
			end
			return g.files_size
		end
	end
	return ui.row {
		h = GROUP_H,
		w_full = true,
		px = 12,
		gap = 10,
		items = "center",
		ui.text { part(1), font = "$font.mono", text_size = 10.5, letter_spacing = 1.2, color = "$text.tertiary" },
		ui.text { part(2), font = "$font.mono", text_size = 10.5, color = "$text.disabled" },
		ui.div { grow = 1, h = 1, bg = "$border.subtle" },
		ui.text { part(3), font = "$font.mono", text_size = 11, color = "$text.tertiary" },
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
	-- A subfolder's count and size, once sizes.rs has them.
	local function info(): any
		local item = r()
		return if item.dir then state.folder_info(item.id) else nil
	end
	local function group(): string?
		return group_at(index:get())
	end
	local line = ui.row(with({
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
			-- The icon and name: a press here is on the item (drag it,
			-- click it); elsewhere on the row it starts a drag-select.
			ui.row {
				min_w = 0,
				shrink = 1,
				gap = 8,
				items = "center",
				on_press = claim_press,
				item_check(function()
					return index:peek()
				end, r),
				common.file_icon(field("icon"), function()
					return r().dir
				end, 18),
				name_cell(r, function()
					return index:peek()
				end),
			},
			-- The extension as a tag, when names hide it.
			ui.show(
				function()
					return (r().ext or "") ~= "" and state.renaming:get() ~= r().id
				end,
				ui.text {
					field("ext"),
					literal = true,
					shrink = 0,
					px = 5,
					rounded = 4,
					bg = "$surface.raised",
					border = 1,
					border_color = "$border.subtle",
					font = "$font.mono",
					text_size = 10,
					color = "$text.secondary",
				}
			),
		},
		ui.row {
			w = col("date"),
			shrink = 0,
			gap = 7,
			items = "center",
			hidden = column_hidden("date"),
			tooltip = ui.text { field("date_full") },
			-- Changed in the last hour.
			ui.show(
				function()
					return r().recent == true
				end,
				ui.div {
					w = 5,
					h = 5,
					shrink = 0,
					rounded = "$radius.full",
					bg = "$accent",
					shadow = { x = 0, y = 0, blur = 6, spread = 0, color = "$app.glow" },
				}
			),
			ui.text {
				field("date"),
				literal = true,
				min_w = 0,
				color = function()
					return if r().recent then "$text.primary" else "$text.secondary"
				end,
				font = "$font.mono",
				text_size = 12,
				truncate = true,
			},
		},
		ui.text {
			function()
				local i = info()
				if i and i.count ~= "" then
					return i.count
				end
				return r().kind
			end,
			literal = true,
			w = col("type"),
			shrink = 0,
			hidden = column_hidden("type"),
			color = function()
				return if r().dir then "$text.tertiary" else "$text.secondary"
			end,
			truncate = true,
		},
		ui.row {
			w = col("size"),
			shrink = 0,
			gap = 8,
			items = "center",
			justify = "end",
			share_leds(function()
				local i = info()
				return if i then i.share else -1
			end),
			ui.text {
				function()
					local i = info()
					if i then
						return i.size
					end
					return r().size
				end,
				literal = true,
				-- A fixed width keeps the LEDs in one column.
				w = 56,
				shrink = 0,
				align = "end",
				color = "$text.secondary",
				font = "$font.mono",
				text_size = 12,
			},
		},
	}, item_handlers(function()
		return index:peek()
	end, function()
		return row:peek()
	end, false)))
	return ui.column {
		w_full = true,
		on_mount = entrance(function()
			return index:peek()
		end),
		ui.show(function()
			return group() ~= nil
		end, group_header(group)),
		line,
	}
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
			item_check(function()
				return index:peek()
			end, r),
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
				local item = r()
				local i = if item.dir then state.folder_info(item.id) else nil
				return if i then i.size else item.size
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
	end, true)))
end

-- Grid ---------------------------------------------------------------------

-- Text wraps only between words, so a long name without spaces
-- ("project_backup_2026") would end at one line. Zero-width spaces inside
-- long words let it break there too: after _ - . where it has them, else
-- every few letters.
local ZWSP = utf8.char(0x200B)
local LONG_WORD = 10

local function soft_breaks(name: string): string
	return (string.gsub(name, "[^%s]+", function(word: string): string?
		local len = utf8.len(word)
		if len == nil or len <= LONG_WORD then
			return nil
		end
		local out = {}
		local run = 0
		for _, code in utf8.codes(word) do
			local ch = utf8.char(code)
			table.insert(out, ch)
			run += 1
			if ch == "_" or ch == "-" or ch == "." or run >= LONG_WORD then
				table.insert(out, ZWSP)
				run = 0
			end
		end
		return table.concat(out)
	end))
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
		relative = true,
		on_mount = entrance(function()
			return i
		end),
		ui.div {
			absolute = true,
			top = 6,
			left = 6,
			item_check(function()
				return i
			end, r),
		},
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
					return if x then soft_breaks(x.name) else ""
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
	end, true)))
end

local function grid_row(r: number): Node
	local cols = state.grid_cols:peek()
	-- Every column gets a cell, shown while its item exists: a row kept as
	-- the list changes (search results streaming in, a folder growing or
	-- shrinking) fills and empties without being built again.
	local cells = {}
	for c = 1, cols do
		local i = (r - 1) * cols + c
		table.insert(cells, ui.show(function()
			return i <= state.rows:len()
		end, grid_cell(i)))
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
			offset = list_offset,
			on_press = list_press,
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
		offset = list_offset,
		on_press = list_press,
		data = state.rows,
		key = keyed,
		row = if kind == "list" then list_row else details_row,
		height = if kind == "list" then LIST_ROW else DETAILS_ROW,
		-- Details: taller rows where the folders and the files start.
		heights = if kind == "list" then nil else row_heights,
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
				if state.rows:len() > 0 then
					return ""
				end
				if n and n.recycle then
					return "The Recycle Bin is empty."
				end
				return if n and n.search then `Nothing here matches “{n.search}”.` else "This folder is empty."
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
		ref = view_ref,
		focusable = true,
		label = "Items",
		on_resize = function(size)
			state.view_w:set(size.w)
		end,
		on_key = on_list_key,
		-- Right-click on empty space: the folder's own menu (New, Paste, ...).
		context_menu = function()
			if item_menu_claimed then
				-- The same right-click, passed on from an item.
				item_menu_claimed = false
				return nil
			end
			ui.invoke("background_menu")
			return nil
		end,
		-- Files dropped on empty space land in this folder: from other apps,
		-- or dragged in Pane from another folder (a folder that sprang open).
		on_file_drop = function(files)
			if state.in_recycle() then
				ui.invoke("recycle", { paths = file_paths(files) })
				return
			end
			drop_on(nil, file_paths(files))
		end,
		drop_when = function(payload: any)
			if type(payload) ~= "table" or payload.paths == nil or not state.nav:peek() then
				return false
			end
			if state.in_recycle() then
				return true
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
			if state.in_recycle() then
				ui.invoke("recycle", { paths = payload.paths })
				return
			end
			drop_on(nil, payload.paths)
		end,
		-- A click on empty space clears the selection.
		on_click = function(e)
			if take_swallowed() then
				return
			end
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
		marquee_box(),
	}
end
