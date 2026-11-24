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

-- A menu entry with a check column.
function M.check_item(label: string, checked: () -> boolean, on_click: () -> ()): Node
	return ui.menu_item {
		label = label,
		on_click = on_click,
		ui.row {
			gap = 8,
			items = "center",
			ui.div {
				w = 14,
				color = "$accent",
				ui.show(checked, icons.icon("check", 14)),
			},
			ui.text { label },
		},
	}
end

function M.separator(): Node
	return ui.div { h = 1, my = 4, bg = "$border.subtle" }
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
