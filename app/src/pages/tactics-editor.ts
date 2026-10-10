import type { ArmyCard, Settings, Tactics } from "../api";
import { validateTactics } from "../tactics";
import { el } from "../dom";

export class TacticsEditor {
  readonly element = el("div", {class:"tactics-editor"});
  private key = "";
  private settings!: Settings;
  private cards: ArmyCard[] = [];
  private busy = false;
  private error = el("p", {role:"alert"});
  constructor(private persist: (next: Partial<Settings>) => Promise<void>) {}

  render(settings: Settings, cards: ArmyCard[], busy: boolean) {
    this.settings=settings; this.cards=cards; this.busy=busy;
    const key=JSON.stringify([settings.tactics,settings.army,busy,cards.length]);
    if (key===this.key) return;
    this.key=key;
    const plan=settings.tactics;
    const enabled=el("input",{type:"checkbox","aria-label":"启用自定义战术"}); enabled.checked=plan.enabled;
    const camera=el("input",{type:"checkbox","aria-label":"战术开始前缩小并向右拖动画面"}); camera.checked=plan.prepareCamera;
    enabled.addEventListener("change",()=>void this.save({...plan,enabled:enabled.checked}));
    camera.addEventListener("change",()=>void this.save({...plan,prepareCamera:camera.checked}));
    const enableLabel=el("label"); enableLabel.append(enabled,document.createTextNode("启用自定义战术"));
    const cameraLabel=el("label"); cameraLabel.append(camera,document.createTextNode("先缩小并向右拖动画面"));
    const toolbar=el("div",{class:"army-toolbar"}); toolbar.append(enableLabel,cameraLabel);
    const list=el("div",{class:"tactic-steps"});
    plan.steps.forEach((step,i)=>{
      const row=el("div",{class:"tactic-step"});
      row.append(el("strong",{text:`${i+1}`}));
      const select=el("select",{"aria-label":`步骤 ${i+1} 卡牌`});
      for (const card of cards) select.append(el("option",{value:card.id,text:`${card.group} · ${card.name}`}));
      select.value=step.card;
      select.addEventListener("change",()=>void this.save({...plan,steps:plan.steps.map((s,n)=>n===i?{...s,card:select.value}:s)}));
      row.append(select);
      for (const [field,label,min,max] of [["count","点击次数",1,400],["x","X",0,1279],["y","Y",40,560],["delayMs","步骤前等待 ms",0,60000],["intervalMs","点击后等待 ms",50,5000]] as const) {
        const wrap=el("label",{text:label});
        const input=el("input",{type:"number",min:String(min),max:String(max),step:"1",value:String(step[field]),"aria-label":`步骤 ${i+1} ${label}`});
        input.addEventListener("change",()=>void this.save({...plan,steps:plan.steps.map((s,n)=>n===i?{...s,[field]:Number(input.value)}:s)}));
        wrap.append(input); row.append(wrap);
      }
      for (const [text,delta] of [["↑",-1],["↓",1]] as const) {
        const button=el("button",{text,"aria-label":`步骤 ${i+1} ${delta<0?"上移":"下移"}`});
        button.disabled=busy || i+delta<0 || i+delta>=plan.steps.length;
        button.addEventListener("click",()=>{const steps=[...plan.steps]; [steps[i],steps[i+delta]]=[steps[i+delta],steps[i]]; void this.save({...plan,steps});}); row.append(button);
      }
      const remove=el("button",{text:"删除","aria-label":`删除步骤 ${i+1}`});
      remove.addEventListener("click",()=>{const steps=plan.steps.filter((_,n)=>n!==i); void this.save({...plan,steps,enabled:steps.length?plan.enabled:false});}); row.append(remove);
      list.append(row);
    });
    const add=el("md-outlined-button",{text:"添加释放步骤"});
    add.toggleAttribute("disabled",busy||plan.steps.length>=32||!cards.length);
    add.addEventListener("click",()=>void this.save({...plan,steps:[...plan.steps,{card:settings.army.entries[0]?.id || cards[0].id,count:1,x:150,y:310,delayMs:0,intervalMs:150}]}));
    this.element.replaceChildren(el("h2",{text:"自定义战术"}),
      el("p",{text:"按顺序选择卡牌并点击固定位置；等待时间从前一步结束开始计算。步骤完成后按设置中的投放策略释放剩余卡牌。"}),
      el("p",{text:"坐标以 1280×720 画面为准，Y 限制在 40～560。点击次数不保证成功投放数量；不可用的卡牌跳过。先添加步骤再启用，停止后编辑，下一次开始生效。"}),
      toolbar,list,add,this.error);
    this.element.querySelectorAll<HTMLInputElement|HTMLSelectElement>("input,select").forEach(e=>e.disabled=busy);
    if (busy) this.element.querySelectorAll<HTMLButtonElement>("button").forEach(e=>e.disabled=true);
  }

  private async save(plan: Tactics) {
    if (this.busy) return;
    try { validateTactics(plan,this.cards); await this.persist({tactics:plan}); this.error.textContent=""; }
    catch (err) { this.error.textContent=String(err); this.key=""; this.render(this.settings,this.cards,this.busy); }
  }
}
