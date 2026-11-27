-- The preview pane beside the file list (Alt+P): the selected item's picture,
-- the start of a text or code file, or its icon, with its details below.
-- Rust publishes what to show (crates/pane-app/src/preview.rs).

local state = require("../state")
local common = require("./common")
local highlight = require("./highlight")

-- Its width is a setting, dragged at its left edge (double click: back
-- to the usual width).
local WIDTH = 320
local MIN_W, MAX_W = 220, 720
local dragging = ui.state("preview_drag", nil :: number?)

local function width(): number
	return dragging:get() or state.setting("preview_width") or WIDTH
end

local function edge(): Node
	local start: { x: number, w: number }? = nil
	local function save(w: number)
		ui.invoke("set_setting", { key = "preview_width", value = math.floor(w + 0.5) })
		ui.after(150, function()
			dragging:set(nil)
		end)
	end
	return ui.div {
		absolute = true,
		left = -4,
		top = 0,
		bottom = 0,
		w = 8,
		cursor = "col_resize",
		label = "Resize preview",
		on_press = function(e)
			start = { x = e.x, w = width() }
		end,
		on_pointer_move = function(e)
			local st = start
			if st then
				dragging:set(math.clamp(st.w - (e.x - st.x), MIN_W, MAX_W))
			end
		end,
		on_release = function()
			local w = dragging:peek()
			start = nil
			if w then
				save(w)
			end
		end,
		on_double_click = function()
			dragging:set(WIDTH)
			save(WIDTH)
		end,
	}
end

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

-- The file's text, coloured by its language when it's code (a textarea
-- made per language: its highlighter is fixed for its life).
local function code_box(lang: string?): Node
	return ui.textarea {
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
		highlight = highlight.for_language(lang),
	}
end

local LANG_NAMES = { "rust", "c", "cs", "js", "go", "python", "shell", "lua", "sql", "config", "json" }

local function text_view(): Node
	local cases: { [string]: Node } = { plain = code_box(nil) }
	for _, l in LANG_NAMES do
		cases[l] = code_box(l)
	end
	return ui.column {
		w_full = true,
		grow = 1,
		min_h = 120,
		gap = 4,
		ui.switch(function()
			return highlight.language(field("name")() or "") or "plain"
		end, cases),
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
		w = width,
		shrink = 0,
		relative = true,
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
		edge(),
	}
end
