-- The sidebar: pinned folders (Quick access), the user's folders, and This
-- PC with its drives. Ctrl+click or middle click opens in a new tab.

local state = require("../state")
local common = require("./common")

local function is_here(path: string): () -> boolean
	return function()
		local n = state.nav:get()
		return n ~= nil and string.lower(n.path) == string.lower(path)
	end
end

local function open_place(path: string)
	return function(e)
		if e and e.ctrl then
			ui.invoke("navigate_new_tab", path)
		else
			ui.invoke("navigate", path)
		end
	end
end

local function item(place: state.Place, opts: { usage: boolean? }?): Node
	local here = is_here(place.path)
	local usage = opts and opts.usage
	return ui.row {
		role = "link",
		label = place.label,
		selected = here,
		w_full = true,
		h = if usage then 44 else 32,
		px = 10,
		gap = 10,
		items = "center",
		rounded = 6,
		bg = function()
			return if here() then "$selection.item" else "#00000000"
		end,
		hover = { bg = "$hover" },
		on_click = open_place(place.path),
		on_aux_click = function(e)
			if e.button == "middle" then
				ui.invoke("navigate_new_tab", place.path)
			end
		end,
		context_menu = function()
			local items = {
				ui.menu_item {
					"Open in new tab",
					on_click = function()
						ui.invoke("navigate_new_tab", place.path)
					end,
				},
			}
			if place.pinned then
				table.insert(
					items,
					ui.menu_item {
						"Unpin from Quick access",
						on_click = function()
							ui.invoke("unpin", place.path)
						end,
					}
				)
			elseif not usage then
				table.insert(
					items,
					ui.menu_item {
						"Pin to Quick access",
						on_click = function()
							ui.invoke("pin", place.path)
						end,
					}
				)
			end
			return items
		end,
		common.file_icon(function()
			return place.icon
		end, function()
			return true
		end, 18),
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 3,
			ui.text { place.label, literal = true, truncate = true },
			ui.show(
				usage == true,
				ui.column {
					gap = 2,
					w_full = true,
					ui.div {
						w_full = true,
						h = 4,
						rounded = 2,
						bg = "$border.subtle",
						overflow = "hidden",
						ui.div {
							h_full = true,
							w = `{math.floor((place.used or 0) * 100)}%`,
							rounded = 2,
							-- Explorer's warning colour once a drive is 90% full.
							bg = if (place.used or 0) > 0.9 then "$danger" else "$accent",
						},
					},
					ui.text {
						place.detail or "",
						literal = true,
						text_size = 11,
						color = "$text.tertiary",
						truncate = true,
					},
				}
			),
		},
	}
end

local function section(title: string, list: () -> { state.Place }, opts: { usage: boolean? }?): Node
	local items = ui.computed(list)
	return ui.show(
		function()
			return #items:get() > 0
		end,
		ui.column {
			w_full = true,
			gap = 1,
			pb = 10,
			ui.text { title, text_size = 11, bold = true, color = "$text.tertiary", px = 10, pt = 4, pb = 4 },
			ui.each(items, function(place: state.Place)
				return item(place, opts)
			end),
		}
	)
end

local function places_field(name: string): () -> { state.Place }
	return function()
		local p = state.places:get()
		return if p then (p :: any)[name] else {}
	end
end

return function(): Node
	return ui.scroll {
		w = 236,
		h_full = true,
		shrink = 0,
		px = 8,
		py = 6,
		gap = 0,
		section("Quick access", places_field("pinned")),
		section("Folders", places_field("known")),
		ui.column {
			w_full = true,
			gap = 1,
			ui.row {
				role = "link",
				label = "This PC",
				w_full = true,
				h = 32,
				px = 10,
				gap = 10,
				items = "center",
				rounded = 6,
				bg = function()
					local n = state.nav:get()
					return if n and n.this_pc then "$selection.item" else "#00000000"
				end,
				hover = { bg = "$hover" },
				on_click = open_place("::thispc"),
				ui.text { "🖥", text_size = 15, w = 18, align = "center" },
				ui.text { "This PC" },
			},
			ui.column {
				w_full = true,
				pl = 12,
				gap = 1,
				ui.each(
					ui.computed(places_field("drives")),
					function(place: state.Place)
						return item(place, { usage = true })
					end
				),
			},
		},
	}
end
