-- Shared building blocks in Pane's design language.

local icons = require("./icons")

local M = {}

-- A square ghost button with one of Pane's icons.
function M.icon_button(props: {
	label: string,
	icon: string,
	on_click: any,
	enabled: (() -> boolean)?,
	active: (() -> boolean)?,
	size: number?,
	icon_size: number?,
}): Node
	local size = props.size or 32
	return ui.button {
		tooltip = props.label,
		-- Below: above would cover the tab strip.
		tooltip_placement = { side = "bottom", align = "center", offset = 6 },
		label = props.label,
		disabled = if props.enabled
			then function()
				return not (props.enabled :: () -> boolean)()
			end
			else false,
		on_click = props.on_click,
		w = size,
		h = size,
		p = 0,
		rounded = "$radius.md",
		bg = if props.active
			then function()
				return if (props.active :: () -> boolean)() then "$surface.raised" else "#00000000"
			end
			else "#00000000",
		color = if props.active
			then function()
				return if (props.active :: () -> boolean)() then "$accent" else "$text.secondary"
			end
			else "$text.secondary",
		hover = { bg = "$hover", color = "$text.primary" },
		active = { bg = "$pressed" },
		items = "center",
		justify = "center",
		shrink = 0,
		icons.icon(props.icon, props.icon_size or 16),
	}
end

-- A file icon from Rust's icon service, with a soft placeholder until it arrives.
function M.file_icon(icon: () -> string, dir: () -> boolean, size: number, large: boolean?): Node
	return ui.image {
		src = function()
			return (if large then "host:l/" else "host:s/") .. icon()
		end,
		w = size,
		h = size,
		fit = "contain",
		shrink = 0,
		fallback = ui.div {
			w = size,
			h = size,
			items = "center",
			justify = "center",
			color = "$text.tertiary",
			icons.icon(if dir() then "folder" else "doc", size * 0.8),
		},
	}
end

-- A grid tile's picture: the file's thumbnail once it is made (host:t/...),
-- its shell icon until then and for files without one.
function M.thumbnail(thumb: () -> string, icon: () -> string, dir: () -> boolean, size: number): Node
	return ui.image {
		src = function()
			local t = thumb()
			return if t ~= "" then "host:" .. t else "host:l/" .. icon()
		end,
		w = size,
		h = size,
		fit = "contain",
		shrink = 0,
		fallback = M.file_icon(icon, dir, size, true),
	}
end

-- A menu entry with a check column (drawn as every menu row: menu_entry).
function M.check_item(label: string, checked: () -> boolean, on_click: () -> ()): Node
	return M.menu_entry(label, on_click, { checked = checked })
end

-- A plain menu entry, lined up with check items (an icon in their column).
function M.menu_item(label: string, on_click: () -> (), icon: string?): Node
	return M.menu_entry(label, on_click, { icon = icon })
end

-- A line between groups in a `ui.menu`.
function M.separator(): Node
	return ui.div { h = 1, my = 4, mx = 6, bg = "$border.subtle" }
end

-- Menus -------------------------------------------------------------------
-- Every menu in Pane (right-click, the toolbar's, the sidebar's) is drawn
-- the same way: a raised warm panel with the machined top edge, rows with
-- an icon column, the label and the key that does it in mono.

-- The panel's look: `context_menu_style` on whatever opens one.
M.menu_style = {
	min_w = 220,
	p = 4,
	gap = 0,
	items = "stretch",
	rounded = "$radius.lg",
	bg = "$surface.overlay",
	border = 1,
	border_color = "$border.subtle",
	shadow = "$shadow.lg",
	inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
}

-- The same look as props for a `ui.menu` / `ui.submenu`.
function M.menu_props(props: { [any]: any }): { [any]: any }
	for k, v in M.menu_style do
		if props[k] == nil then
			props[k] = v
		end
	end
	return props
end

-- One row's look: its height, padding and hover.
local function entry_look(props: { [any]: any }): { [any]: any }
	props.h = 28
	props.px = 8
	props.rounded = "$radius.md"
	props.text_size = 13
	props.color = "$text.primary"
	props.bg = "#00000000"
	props.hover = { bg = "$hover" }
	props.focus = { bg = "$hover", border = 0 }
	return props
end

export type EntryOptions = {
	-- One of Pane's icons, in the icon column.
	icon: string?,
	-- A check in the icon column while true (instead of the icon).
	checked: (() -> boolean)?,
	-- The key that does it ("Ctrl+C"), shown at the end.
	key: (() -> string)?,
	-- Destructive: the icon and label in the danger colour.
	danger: boolean?,
	-- Ends in a "›" (a submenu's row).
	more: boolean?,
}

-- A menu row: [icon] label ........ key
function M.menu_entry(label: string, on_click: (() -> ())?, opts: EntryOptions?): Node
	local o: EntryOptions = opts or {}
	local tint = if o.danger then "$danger" else "$text.secondary"
	local mark: Node = if o.checked
		then ui.div {
			w = 14,
			color = "$accent",
			ui.show(o.checked, icons.icon("check", 14)),
		}
		else ui.div {
			w = 14,
			color = tint,
			ui.show(o.icon ~= nil, icons.icon(o.icon or "dots", 14)),
		}
	return ui.menu_item(entry_look {
		label = label,
		on_click = on_click,
		ui.row {
			gap = 9,
			items = "center",
			w_full = true,
			mark,
			ui.text { label, grow = 1, color = if o.danger then "$danger" else "$text.primary" },
			if o.key
				then ui.text {
					o.key,
					font = "$font.mono",
					text_size = 11,
					color = "$text.tertiary",
					pl = 20,
				}
				else nil,
			if o.more then ui.div { color = "$text.tertiary", pl = 12, icons.icon("chevron", 12) } else nil,
		},
	})
end

-- A submenu: a row with "›" that opens `items` beside it. `open` is a
-- state the menu writes back as it closes.
function M.menu_sub(label: string, open: any, items: { Node }, icon: string?): Node
	local props = M.menu_props {
		open = open,
		label = label,
		trigger = M.menu_entry(label, nil, { icon = icon, more = true }),
	}
	for _, it in items do
		table.insert(props, it)
	end
	return ui.submenu(props)
end

-- A line between groups of rows.
function M.menu_line(): Node
	return ui.separator { h = 1, my = 4, mx = 6, bg = "$border.subtle" }
end

-- A raised card with Pane's machined top edge.
function M.card(props: { [string]: any }): Node
	local node = table.clone(props)
	node.bg = node.bg or "$surface.raised"
	node.rounded = node.rounded or "$radius.lg"
	node.border = node.border or 1
	node.border_color = node.border_color or "$border.subtle"
	node.inset_shadow = node.inset_shadow or { x = 0, y = 1, color = "$app.highlight" }
	return ui.column(node)
end

-- A small mono caption ("LABEL" style) for section headers.
function M.caption(text: string): Node
	return ui.text {
		string.upper(text),
		font = "$font.mono",
		text_size = 10.5,
		letter_spacing = 1.2,
		color = "$text.tertiary",
	}
end

-- The amber LED pip at a selected row's left edge.
function M.pip(on: () -> boolean): Node
	return ui.div {
		absolute = true,
		left = 0,
		top = 7,
		bottom = 7,
		w = 3,
		rounded = "$radius.full",
		bg = "$app.pip",
		shadow = { x = 0, y = 0, blur = 8, spread = 0, color = "$app.glow" },
		opacity = function()
			return if on() then 1 else 0
		end,
	}
end

return M
