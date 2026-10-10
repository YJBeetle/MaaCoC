import { test } from "node:test";
import assert from "node:assert/strict";
import { encodeArmy, decodeArmy } from "../src/army-share.ts";
const cards = [{id:"Unit/114_TotemThrower",group:"Unit",name:"TotemThrower",preview:""}];
const plan = {army:{entries:[{id:cards[0].id,count:5}]},strategy:"air-probe" as const};
test("分享码只传递卡组和策略，导入前完整校验", () => {
  assert.deepEqual(decodeArmy(encodeArmy(plan),cards),plan);
  assert.throws(() => decodeArmy("MC2:anything",cards));
  assert.throws(() => decodeArmy("MC1:broken",cards));
  assert.throws(() => decodeArmy(encodeArmy({...plan,army:{entries:[...plan.army.entries,...plan.army.entries]}}),cards));
  assert.throws(() => decodeArmy(encodeArmy({...plan,army:{entries:[{id:cards[0].id,count:-1}]}}),cards));
  assert.throws(() => decodeArmy(encodeArmy(plan),[]));
});

test("分享编号与卡牌目录排序无关，校验码拒绝文本改动", () => {
  const code = encodeArmy(plan);
  assert.ok(code.length < 30);
  assert.deepEqual(decodeArmy(code,[...cards].reverse()), plan);
  assert.throws(() => decodeArmy(code.replace(".5", ".6"),cards));
});
