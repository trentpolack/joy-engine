-- Copyright (c) 2026 Trent Polack. All Rights Reserved.
-- Licensed under the MIT License.

-- Backing tables live only in these closures. Weak keys allow an unused frozen graph, including its cycles,
-- to be collected. In-place proxies preserve aliases captured by module functions and table-key identities.
local table_getmetatable, table_setmetatable, array_metatable = ...
local real_rawget, real_rawset, real_rawlen = rawget, rawset, rawlen
local real_next, real_getmetatable, real_setmetatable = next, getmetatable, setmetatable
local backings = real_setmetatable({}, {__mode = 'k'})
local function readonly()
    error('attempt to modify a readonly table', 2)
end

local function freeze(root)
    local pending, tables, seen = {root}, {}, {}
    while #pending > 0 do
        local value = pending[#pending]
        pending[#pending] = nil
        if not seen[value] and not backings[value] then
            seen[value] = true
            tables[#tables + 1] = value
            for key, entry in real_next, value do
                if type(key) == 'table' then pending[#pending + 1] = key end
                if type(entry) == 'table' then pending[#pending + 1] = entry end
            end
            local metatable = table_getmetatable(value)
            if metatable ~= nil and metatable ~= array_metatable then pending[#pending + 1] = metatable end
        end
    end

    -- Snapshot before transforming any member; metatables can themselves belong to the graph.
    local metatables = {}
    for _, value in ipairs(tables) do
        local backing = {}
        for key, entry in real_next, value do real_rawset(backing, key, entry) end
        local metatable = table_getmetatable(value)
        metatables[value] = metatable
        table_setmetatable(backing, metatable)
        backings[value] = backing
    end
    for _, value in ipairs(tables) do
        local backing = backings[value]
        local old = metatables[value]
        local metatable = {}
        if old then
            for key, entry in real_next, backings[old] or old do
                real_rawset(metatable, key, entry)
            end
        end
        local old_index = metatable.__index
        metatable.__index = function(_, key)
            local entry = real_rawget(backing, key)
            if entry ~= nil then return entry end
            if type(old_index) == 'function' then return old_index(value, key) end
            if type(old_index) == 'table' then return old_index[key] end
        end
        metatable.__newindex = readonly
        metatable.__len = function() return real_rawlen(backing) end
        metatable.__metatable = false
        metatable.__pairs = function()
            -- Never return the backing as the iterator state: callers can inspect every returned value.
            return function(_, key) return real_next(backing, key) end, nil, nil
        end
        for key in real_next, value do real_rawset(value, key, nil) end
        table_setmetatable(value, metatable)
    end
    return root
end

-- Lua disables hooks while executing garbage-collection finalizers, so authored finalizers cannot be bounded.
setmetatable = function(value, metatable)
    if type(metatable) == 'table' and real_rawget(backings[metatable] or metatable, '__gc') ~= nil then
        error('authored garbage-collection finalizers are unavailable', 2)
    end
    return real_setmetatable(value, metatable)
end

rawset = function(value, key, entry)
    if backings[value] then readonly() end
    return real_rawset(value, key, entry)
end
rawget = function(value, key) return real_rawget(backings[value] or value, key) end
rawlen = function(value) return real_rawlen(backings[value] or value) end
next = function(value, key) return real_next(backings[value] or value, key) end

-- Strings share one VM metatable even across private script environments.
local string_metatable = real_getmetatable('')
if string_metatable then real_rawset(string_metatable, '__metatable', false) end

return {
    freeze = freeze,
    backing = function(value) return backings[value] or value end,
}
