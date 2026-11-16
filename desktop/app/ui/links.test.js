const assert = require("assert"), { linkAt } = require("./links.js");
const t = "veja https://a.com/x?y=1. e C:\\Users\\me\\a.txt, fim";
assert.strictEqual(linkAt(t, 6), "https://a.com/x?y=1");
assert.strictEqual(linkAt(t, 24), null);
assert.strictEqual(linkAt(t, 30), "C:\\Users\\me\\a.txt");
assert.strictEqual(linkAt("nada aqui", 2), null);
console.log("ok");
