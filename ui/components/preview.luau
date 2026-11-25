-- The preview pane beside the file list (Alt+P): the selected item's picture,
-- the start of a text or code file, or its icon, with its details below.
-- Rust publishes what to show (crates/pane-app/src/preview.rs).

local state = require("../state")
local common = require("./common")

local WIDTH = 320

local function p(): any
	return state.preview:get()
end

local function field(name: string): () -> any
	return function()
		local x = p()
		return if x then x[name] else nil
	end
end

local function is_content(kind: string): () -> boolean
	return function()
		local x = p()
		return x ~= nil and x.state == "item" and x.content == kind
	end
end

-- A sentence in the middle of the pane: nothing selected, or several.
local function message(): Node
	return ui.column {
		grow = 1,
		w_full = true,
		items = "center",
		justify = "center",
		px = 24,
		ui.text {
			function()
				local x = p()
				if x and x.state == "many" then
					return `{x.count} items selected`
				end
				return "Select a file to preview."
			end,
			color = "$text.tertiary",
			align = "center",
			wrap = true,
		},
	}
end

local function picture(): Node
	return ui.div {
		w_full = true,
		h = 240,
		shrink = 0,
		items = "center",
		justify = "center",
		rounded = "$radius.md",
		bg = "$surface.sunken",
		overflow = "hidden",
		ui.switch(function()
			local x = p()
			return if x and x.content == "image" then "image" else "icon"
		end, {
			image = ui.image {
				src = function()
					return field("image")() or ""
				end,
				w_full = true,
				h_full = true,
				fit = "contain",
			},
			icon = common.file_icon(function()
				return field("icon")() or ""
			end, function()
				return field("dir")() == true
			end, 96, true),
		}),
	}
end

local function text_view(): Node
	return ui.column {
		w_full = true,
		grow = 1,
		min_h = 120,
		gap = 4,
		ui.textarea {
			label = "File contents",
			value = function()
				return field("text")() or ""
			end,
			read_only = true,
			line_numbers = true,
			font = "$font.mono",
			text_size = 12,
			w_full = true,
			grow = 1,
			min_h = 0,
			bg = "$surface.sunken",
			overscroll = "contain",
		},
		ui.show(function()
			return field("truncated")() == true
		end, ui.text {
			function()
				return `Showing the first {state.setting("preview_max_kb") or 1024} KB.`
			end,
			color = "$text.tertiary",
			text_size = "$font.size.sm",
		}),
	}
end

local function details(): Node
	local rows = ui.computed(function(): { any }
		local x = p()
		return if x and x.details then x.details else {}
	end)
	return ui.column {
		w_full = true,
		gap = 6,
		shrink = 0,
		ui.text {
			function()
				return field("name")() or ""
			end,
			literal = true,
			weight = 600,
			wrap = true,
			w_full = true,
		},
		ui.each(rows, function(row: any)
			return ui.row {
				w_full = true,
				gap = 8,
				items = "start",
				ui.text { row.label, w = 76, shrink = 0, color = "$text.tertiary", text_size = "$font.size.sm" },
				ui.text {
					row.value,
					literal = true,
					grow = 1,
					min_w = 0,
					wrap = true,
					color = "$text.secondary",
					text_size = "$font.size.sm",
				},
			}
		end),
	}
end

local function item(): Node
	return ui.column {
		grow = 1,
		w_full = true,
		min_h = 0,
		p = 14,
		gap = 14,
		ui.show(function()
			return not is_content("text")()
		end, picture()),
		ui.show(is_content("text"), text_view()),
		details(),
	}
end

return function(): Node
	return ui.column {
		w = WIDTH,
		shrink = 0,
		h_full = true,
		min_h = 0,
		role = "group",
		label = "Preview",
		border_sides = { 0, 0, 0, 1 },
		border_color = "$border.subtle",
		ui.show(function()
			local x = p()
			return x ~= nil and x.state == "item"
		end, item(), message()),
	}
end
