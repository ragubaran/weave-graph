require("json")

local function formatName(name)
    return name:upper()
end

function greet(name)
    return formatName(name)
end

greet("world")
