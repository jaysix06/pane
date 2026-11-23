-- Small building blocks shared by the components.

local M = {}

-- A square toolbar button with a glyph.
function M.icon_button(props: {
	label: string,
	glyph: string,
	on_click: any,
	enabled: (() -> boolean)?,
	size: number?,
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
		rounded = 6,
		bg = "#00000000",
		hover = { bg = "$hover" },
		items = "center",
		justify = "center",
		shrink = 0,
		ui.text { props.glyph, text_size = 15 },
	}
end

-- A file icon from Rust's icon service, with a glyph until it arrives.
function M.file_icon(icon: () -> string, dir: () -> boolean, size: number, large: boolean?): Node
	return ui.image {
		src = function()
			return (if large then "host:l/" else "host:s/") .. icon()
		end,
		w = size,
		h = size,
		fit = "contain",
		shrink = 0,
		fallback = ui.text {
			function()
				return if dir() then "📁" else "📄"
			end,
			text_size = size * 0.8,
			w = size,
			h = size,
			align = "center",
		},
	}
end

-- A menu entry with a check mark column.
function M.check_item(label: string, checked: () -> boolean, on_click: () -> ()): Node
	return ui.menu_item {
		label = label,
		on_click = on_click,
		ui.row {
			gap = 8,
			items = "center",
			ui.text {
				function()
					return if checked() then "✓" else ""
				end,
				w = 14,
				color = "$accent",
			},
			ui.text { label },
		},
	}
end

function M.separator(): Node
	return ui.div { h = 1, my = 4, bg = "$border.subtle" }
end

return M
