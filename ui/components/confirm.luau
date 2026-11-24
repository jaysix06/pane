-- A confirmation dialog in Pane's style. Callers ask with plain data (an
-- action name and its argument), which runs only if the person confirms:
-- permanent deletions and cleanups always come through here.

local icons = require("./icons")

local M = {}

export type Ask = {
	title: string,
	body: string,
	label: string,
	danger: boolean?,
	action: string,
	arg: any?,
}

M.open = ui.state("confirm_open", false)
local data = ui.state("confirm_data", nil :: Ask?)

function M.ask(a: Ask)
	data:set(a)
	M.open:set(true)
end

local function field(name: string): () -> any
	return function()
		local d = data:get()
		return if d then (d :: any)[name] else nil
	end
end

local function confirm()
	local d = data:peek()
	M.open:set(false)
	if d then
		ui.invoke(d.action, d.arg)
	end
end

function M.dialog(): Node
	return ui.modal {
		open = M.open,
		label = "Confirm",
		w = 420,
		p = 0,
		bg = "#00000000",
		build = function()
			local danger = (data:peek() or ({} :: any)).danger == true
			return ui.column {
				w = 420,
				p = 22,
				gap = 16,
				bg = "$surface.overlay",
				rounded = "$radius.xl",
				border = 1,
				border_color = "$border.subtle",
				shadow = "$shadow.lg",
				ui.row {
					gap = 12,
					items = "start",
					ui.div {
						w = 34,
						h = 34,
						rounded = "$radius.md",
						bg = if danger then "#FF5D4D24" else "$selection.item",
						items = "center",
						justify = "center",
						shrink = 0,
						icons.icon(if danger then "warning" else "trash", 18, if danger then "$danger" else "$accent"),
					},
					ui.column {
						grow = 1,
						min_w = 0,
						gap = 6,
						ui.text { field("title"), weight = 600, text_size = 16 },
						ui.text { field("body"), color = "$text.secondary", wrap = true, literal = true },
					},
				},
				ui.row {
					w_full = true,
					gap = 8,
					justify = "end",
					ui.button {
						label = "Cancel",
						on_click = ui.set(M.open, false),
						px = 14,
						h = 32,
						rounded = "$radius.md",
						bg = "$surface.raised",
						border = 1,
						border_color = "$border.subtle",
						hover = { border_color = "$border.strong" },
						ui.text { "Cancel" },
					},
					ui.button {
						label = function()
							return field("label")() or "OK"
						end,
						on_click = confirm,
						-- Enter presses the focused button: the action for
						-- ordinary confirmations, Cancel for dangerous ones.
						on_mount = function(el)
							if not danger then
								el:focus()
							end
						end,
						px = 14,
						h = 32,
						rounded = "$radius.md",
						bg = if danger then "$danger" else "$accent",
						color = if danger then "#FFFFFF" else "$accent.text",
						hover = { opacity = 0.9 },
						ui.text { field("label"), weight = 600 },
					},
				},
			}
		end,
	}
end

return M
