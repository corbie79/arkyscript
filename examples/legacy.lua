-- Ordinary Lua: no annotations or frontend-specific syntax required.
local player = { name = "Mina", score = 10 }
function player:add(amount)
   self.score = self.score + amount
end
player:add(5)
print(player.name .. ": " .. player.score)
