-- Copyright (c) 2026 Trent Polack. All Rights Reserved.
-- Licensed under the MIT License.

-- Lua errors are catchable, including count-hook failures inside nonyieldable C callbacks. Check every protected
-- boundary before its handler or continuation can run, so exhaustion unwinds all the way to the host.
local check_budget = ...
local protected_call, protected_handler = pcall, xpcall
local pack, unpack = table.pack, table.unpack
pcall = function(callback, ...)
    check_budget()
    local results = pack(protected_call(callback, ...))
    check_budget()
    return unpack(results, 1, results.n)
end
xpcall = function(callback, handler, ...)
    check_budget()
    if type(handler) ~= 'function' then error("bad argument #2 to 'xpcall' (function expected)", 2) end
    local function checked_handler(message)
        check_budget()
        return handler(message)
    end
    local results = pack(protected_handler(callback, checked_handler, ...))
    check_budget()
    return unpack(results, 1, results.n)
end
