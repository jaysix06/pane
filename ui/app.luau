-- Pane: milestone 1 spike. A toolbar in the title bar strip, a details
-- view on a virtual list fed by Rust, and Explorer's right-click menu.

ui.theme.follow_system { dark = "zgx-dark", light = "zgx-light" }

type Row = { id: string, name: string, dir: boolean, kind: string, date: string, size: string }
type Crumb = { label: string, path: string }
type Nav = { can_back: boolean, can_forward: boolean, can_up: boolean, crumbs: { Crumb } }

-- Published by Rust (crates/pane-app/src/main.rs).
local rows = ui.shared("rows", {} :: { Row })
local nav = ui.shared("nav", nil :: Nav?)
local status = ui.shared("status", "")

local selected = ui.state("selected", 0)
local crumbs = ui.computed(function(): { Crumb }
	local n = nav:get()
	return if n then n.crumbs else {}
end)

local ROW_H = 30
local COL_DATE = 170
local COL_TYPE = 150
local COL_SIZE = 90

local function nav_flag(name: string): () -> boolean
	return function()
		local n = nav:get()
		return n ~= nil and (n :: any)[name] == true
	end
end

local function tool_button(label: string, glyph: string, action: string, enabled: () -> boolean): Node
	return ui.button {
		tooltip = label,
		label = label,
		disabled = function()
			return not enabled()
		end,
		on_click = ui.action(action),
		w = 32,
		h = 32,
		p = 0,
		rounded = 6,
		bg = "#00000000",
		hover = { bg = "$hover" },
		items = "center",
		justify = "center",
		ui.text { glyph, text_size = 16 },
	}
end

local function breadcrumbs(): Node
	return ui.row {
		grow = 1,
		min_w = 0,
		h = 32,
		px = 8,
		gap = 2,
		items = "center",
		rounded = 6,
		bg = "$surface.raised",
		border = 1,
		border_color = "$border.subtle",
		overflow = "hidden",
		ui.each(crumbs, function(crumb: Crumb, index: number)
			return ui.row {
				gap = 2,
				items = "center",
				shrink = 0,
				ui.show(index > 1, ui.text { "›", color = "$text.tertiary", px = 2 }),
				ui.button {
					label = crumb.label,
					on_click = ui.action("navigate", crumb.path),
					px = 6,
					py = 2,
					rounded = 4,
					bg = "#00000000",
					hover = { bg = "$hover" },
					ui.text { crumb.label, literal = true },
				},
			}
		end),
	}
end

local function toolbar(): Node
	return ui.row {
		h = 48,
		w_full = true,
		window_drag = true,
		items = "center",
		gap = 4,
		pl = 10,
		-- Clear of Windows' minimize / maximize / close at the top right.
		pr = function()
			local area, buttons = ui.window.titlebar_area:get(), ui.window.controls:get()
			return if area and buttons and buttons.x >= area.x + area.width then buttons.width + 12 else 12
		end,
		tool_button("Back (Alt+Left)", "←", "back", nav_flag("can_back")),
		tool_button("Forward (Alt+Right)", "→", "forward", nav_flag("can_forward")),
		tool_button("Up (Alt+Up)", "↑", "up", nav_flag("can_up")),
		tool_button("Refresh (F5)", "⟳", "refresh", function()
			return true
		end),
		ui.div { w = 6 },
		breadcrumbs(),
	}
end

local function header_cell(label: string, width: number?, align: string?): Node
	return ui.text {
		label,
		w = width,
		grow = if width then 0 else 1,
		shrink = if width then 0 else 1,
		min_w = 0,
		align = align,
		color = "$text.secondary",
		text_size = "$font.size.sm",
		truncate = true,
	}
end

local function header(): Node
	return ui.row {
		h = 30,
		w_full = true,
		px = 16,
		gap = 12,
		items = "center",
		border_sides = { 0, 0, 1, 0 },
		border_color = "$border.subtle",
		header_cell("Name"),
		header_cell("Date modified", COL_DATE),
		header_cell("Type", COL_TYPE),
		header_cell("Size", COL_SIZE, "end"),
	}
end

local function file_row(item: Row, index: State<number>, row: State<Row>): Node
	local function field(name: string): () -> string
		return function()
			return (row:get() :: any)[name]
		end
	end
	local function is_selected(): boolean
		return selected:get() == index:get()
	end
	return ui.row {
		role = "option",
		label = field("name"),
		selected = is_selected,
		w_full = true,
		h = ROW_H,
		px = 10,
		mx = 6,
		gap = 12,
		items = "center",
		rounded = 4,
		bg = function()
			return if is_selected() then "$selection.item" else "#00000000"
		end,
		hover = { bg = "$hover" },
		on_click = function()
			selected:set(index:peek())
		end,
		on_double_click = function()
			ui.invoke("open", index:peek())
		end,
		context_menu = function()
			selected:set(index:peek())
			ui.invoke("context_menu", index:peek())
			-- Explorer's own menu was shown natively; no zgx menu.
			return nil
		end,
		ui.row {
			grow = 1,
			min_w = 0,
			gap = 8,
			items = "center",
			ui.text {
				function()
					return if row:get().dir then "📁" else "📄"
				end,
				w = 18,
				shrink = 0,
				text_size = 14,
			},
			ui.text { field("name"), literal = true, truncate = true, min_w = 0 },
		},
		ui.text { field("date"), literal = true, w = COL_DATE, shrink = 0, color = "$text.secondary", truncate = true },
		ui.text { field("kind"), literal = true, w = COL_TYPE, shrink = 0, color = "$text.secondary", truncate = true },
		ui.text { field("size"), literal = true, w = COL_SIZE, shrink = 0, align = "end", color = "$text.secondary" },
	}
end

local function file_list(): Node
	return ui.list {
		data = rows,
		key = function(item: Row)
			return item.id
		end,
		row = file_row,
		height = ROW_H,
		keyboard = true,
		scrollbar = true,
		w_full = true,
		grow = 1,
		py = 4,
	}
end

local function status_bar(): Node
	return ui.row {
		h = 28,
		w_full = true,
		px = 16,
		items = "center",
		border_sides = { 1, 0, 0, 0 },
		border_color = "$border.subtle",
		ui.text { function()
			return status:get()
		end, color = "$text.secondary", text_size = "$font.size.sm", truncate = true },
	}
end

return ui.column {
	w_full = true,
	h_full = true,
	on_key = function(e)
		if e.editing then
			return false
		end
		if e.alt and e.key == "ArrowLeft" then
			ui.invoke("back")
		elseif e.alt and e.key == "ArrowRight" then
			ui.invoke("forward")
		elseif (e.alt and e.key == "ArrowUp") or e.key == "Backspace" then
			ui.invoke("up")
		elseif e.key == "F5" then
			ui.invoke("refresh")
		elseif e.key == "Enter" and selected:peek() > 0 then
			ui.invoke("open", selected:peek())
		else
			return false
		end
		return true
	end,
	toolbar(),
	ui.column {
		grow = 1,
		w_full = true,
		min_h = 0,
		mx = 8,
		mb = 8,
		rounded = 8,
		bg = "$surface.base",
		border = 1,
		border_color = "$border.subtle",
		overflow = "hidden",
		header(),
		file_list(),
		status_bar(),
	},
}
