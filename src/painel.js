// Comportamento da cápsula e do cartão: dados do contrato, três estados, abas, busca, abrir no Gestta.
// Usado por painel.html (janela da casca) e index.html (protótipo de revisão no navegador).
import { tabsOf } from './api/client.js';
import { api, abrirNoNavegador, ajustarJanela, arrastarJanela, posicaoJanela, aoMudarPosicao, avisarEvento } from './transporte.js';

var cfg={poll_interval_seconds:600,morning:{enabled:true,duration_seconds:10,once_per_day:true,not_before:'07:00'}};

/* ---------- formatação (dado já resolvido pelo servidor; aqui só texto) ---------- */
var DOW=['dom','seg','ter','qua','qui','sex','sáb'];
var DOW_LONG=['Domingo','Segunda','Terça','Quarta','Quinta','Sexta','Sábado'];
var MES=['janeiro','fevereiro','março','abril','maio','junho','julho','agosto','setembro','outubro','novembro','dezembro'];
function pad(n){return (n<10?'0':'')+n}
function ymd(s){var p=s.split('-');return new Date(+p[0],+p[1]-1,+p[2]);}
function cap(s){return s.charAt(0).toUpperCase()+s.slice(1)}
function esc(s){return String(s==null?'':s).replace(/[&<>"]/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c];});}
function hhmm(d){return pad(d.getHours())+':'+pad(d.getMinutes())}
function ddmm(d){return pad(d.getDate())+'/'+pad(d.getMonth()+1)}
function hour(t){var p=t.split(':');return (+p[0])+'h'+(p[1]==='00'?'':p[1]);}
function tagOf(t){
  if(t.status==='done')return 'ok';
  if(t.bucket==='overdue')return t.days_overdue+'d';
  if(t.bucket==='today')return t.due_time?hour(t.due_time):'hoje';
  if(!t.due_date)return 'sem data';
  var d=ymd(t.due_date);
  if(t.bucket==='week')return DOW[d.getDay()]+' '+d.getDate();
  return ddmm(d);
}
function rowHtml(t){
  var cls='task'+(t.status==='done'?' ok':t.bucket==='overdue'?' ov':t.bucket==='later'?' far':'');
  var sub=esc(t.client)+(t.competence&&t.title.indexOf('comp.')<0?' · comp. '+esc(t.competence.slice(5)+'/'+t.competence.slice(0,4)):'')+(t.status==='impediment'?' · impedida':'');
  return '<div class="'+cls+'" tabindex="0" data-id="'+esc(t.id)+'" data-n="'+esc((t.title+' '+t.client).toLowerCase())+'"><span class="dot"></span><div class="tm"><b>'+esc(t.title)+'</b><small>'+sub+'</small></div><span class="tag">'+tagOf(t)+'<span class="go">↗</span></span></div>';
}
function group(title,list,cls){return '<div class="g'+(cls?' '+cls:'')+'"><h2>'+title+' <span class="c">'+list.length+'</span></h2>'+list.map(rowHtml).join('')+'</div>';}

/* ---------- render ---------- */
var byId={},all=[],data=null,stale=false;
function render(d){
  if(!d)return; // 304: nada mudou
  data=d;byId={};d.tasks.forEach(function(t){byId[t.id]=t;});
  var s=d.summary,today=ymd(d.today);
  document.getElementById('greetName').textContent='Bom dia, '+d.person.first_name;
  document.getElementById('greetDate').textContent=DOW_LONG[today.getDay()]+', '+today.getDate()+' de '+MES[today.getMonth()];
  // cápsula e progresso
  document.getElementById('capOv').textContent=s.overdue;
  document.getElementById('capOv').parentNode.classList.toggle('zero',!s.overdue);
  document.getElementById('capHoje').textContent=s.due_today;
  var pct=s.today_total?s.done_today/s.today_total:0;
  document.getElementById('ringFg').setAttribute('stroke-dashoffset',(47.1*(1-pct)).toFixed(1));
  document.getElementById('progTxt').textContent=s.done_today+' de '+s.today_total;
  document.getElementById('progBar').style.width=Math.round(pct*100)+'%';
  var tb=tabsOf(d);
  // Hoje = vencidas + hoje (abertas antes das concluídas)
  document.getElementById('cHoje').textContent=tb.hoje.count;
  document.getElementById('pane-hoje').innerHTML=(tb.hoje.overdue.length?group('Vencidas',tb.hoje.overdue,'v'):'')+(tb.hoje.today.length?group('Hoje',tb.hoje.today):'')||'<div class="empty">Nada pra hoje.</div>';
  // Semana, por dia
  var groups={},order=[];
  tb.semana.tasks.forEach(function(t){if(!groups[t.due_date]){groups[t.due_date]=[];order.push(t.due_date);}groups[t.due_date].push(t);});
  document.getElementById('pane-semana').innerHTML=order.map(function(k){var x=ymd(k);return group(cap(DOW[x.getDay()])+' '+x.getDate(),groups[k]);}).join('')||'<div class="empty">Nada agendado pra semana.</div>';
  document.getElementById('cSemana').textContent=tb.semana.count;
  // Todas = não concluídas
  all=tb.todas.tasks;
  document.getElementById('todasList').innerHTML=all.map(rowHtml).join('')+'<div class="empty" id="noRes" hidden>Nada com esse nome.</div>';
  document.getElementById('cTodas').textContent=tb.todas.count;
  filter();
  // frescor
  var last=new Date(d.sync.last_sync_at);
  stale=!!d.sync.stale;
  var dias=Math.round((today-new Date(last.getFullYear(),last.getMonth(),last.getDate()))/864e5);
  var quando=dias===0?'das '+hhmm(last):dias===1?'de ontem':'de '+ddmm(last);
  document.getElementById('stampTxt').textContent=stale?'dados '+quando:'atualizado '+hhmm(last);
  document.getElementById('bannerTxt').innerHTML='<b>Dados '+quando+'.</b> Última sincronização '+ddmm(last)+', '+hhmm(last)+'.';
  document.getElementById('foot').textContent=stale?'ÚLTIMO SYNC '+ddmm(last)+' '+hhmm(last):cadencia();
  applyMode();
}
// Rodapé: na v1 é só 'GESTTA' (decisão do José Luiz, 08/10). cfg.sync_times já chega e fica guardado em cfg;
// 'próxima atualização às HH:MM' só entra quando ele confirmar os horários do robô no Hub — não pela presença do campo.
function cadencia(){return 'GESTTA';}
// Erros que significam "esta máquina precisa de código novo": não adianta tentar de novo.
// not_enrolled = sem token no keychain (casca); unauthorized / token_revoked = 401 do Hub (contrato §1.4).
var NAO_CONECTADO={not_enrolled:1,unauthorized:1,token_revoked:1};
function semDado(){
  // máquina não conectada: nada de número, nem os que já estavam na tela
  data=null;all=[];byId={};
  ['capOv','capHoje','cHoje','cSemana','cTodas','progTxt'].forEach(function(id){document.getElementById(id).textContent='–';});
  document.getElementById('ringFg').setAttribute('stroke-dashoffset','47.1');
  document.getElementById('progBar').style.width='0%';
  ['pane-hoje','pane-semana','todasList'].forEach(function(id){document.getElementById(id).innerHTML='';});
  qn.textContent='';
}
function fail(err){
  var code=err&&err.code;
  console.warn('gestor api',code,err);
  stale=true;
  if(NAO_CONECTADO[code]){
    semDado();
    document.getElementById('stampTxt').textContent='não conectado';
    document.getElementById('bannerTxt').innerHTML='<b>Este computador ainda não foi conectado ao Ápice Hub.</b> Peça o código ao administrador.';
    document.getElementById('foot').textContent='NÃO CONECTADO';
  }else if(!data){
    // sem resposta do Hub: se já temos dado, ele vira "velho"; se não, avisa sem inventar números
    document.getElementById('stampTxt').textContent='sem conexão';
    document.getElementById('bannerTxt').innerHTML='<b>Sem conexão com o Hub.</b> Tentando de novo.';
    document.getElementById('foot').textContent='SEM SYNC';
  }
  // sem dado não é alarme (José Luiz, 08/10): o traço da cápsula fica cinza, como o zero
  if(!data)document.getElementById('capOv').parentNode.classList.add('zero');
  applyMode();
}
function load(){return api.getTasks().then(render,fail);}

/* ---------- busca (Todas) ---------- */
var q=document.getElementById('q'),qn=document.getElementById('qn');
function filter(){
  var v=q.value.trim().toLowerCase(),n=0,rowsEl=document.getElementById('todasList').children;
  for(var i=0;i<rowsEl.length;i++){var el=rowsEl[i];if(!el.classList.contains('task'))continue;var ok=!v||el.getAttribute('data-n').indexOf(v)>=0;el.hidden=!ok;if(ok)n++;}
  var nr=document.getElementById('noRes');if(nr)nr.hidden=n>0;
  var tot=data?data.summary.open_total:all.length;
  qn.textContent=v?(n+' de '+all.length):(data&&data.truncated?'mostrando '+all.length+' de '+tot:tot+' abertas');
}
q.addEventListener('input',filter);

/* ---------- tabs ---------- */
var tabs=document.querySelectorAll('.tabs [role=tab]');
function showTab(id){
  for(var i=0;i<tabs.length;i++){var on=tabs[i].getAttribute('data-tab')===id;tabs[i].setAttribute('aria-selected',on?'true':'false');}
  var panes=document.querySelectorAll('.pane');for(var j=0;j<panes.length;j++)panes[j].classList.toggle('on',panes[j].id==='pane-'+id);
}
document.querySelector('.tabs').addEventListener('click',function(e){var b=e.target.closest('[role=tab]');if(b)showTab(b.getAttribute('data-tab'));});

/* ---------- abrir no Gestta (só leitura: só abre o link) ---------- */
var toast=document.getElementById('toast'),tt=null;
function openTask(el){
  var t=byId[el.getAttribute('data-id')];if(!t)return;
  toast.textContent='Abrindo no Gestta ↗  '+t.title;toast.classList.add('show');
  clearTimeout(tt);tt=setTimeout(function(){toast.classList.remove('show')},1600);
  if(t.gestta_url)abrirNoNavegador(t.gestta_url).catch(function(e){console.warn('abrir no Gestta',e);});
}
document.getElementById('card').addEventListener('click',function(e){var t=e.target.closest('.task');if(t)openTask(t);});
document.getElementById('card').addEventListener('keydown',function(e){if(e.key==='Enter'){var t=e.target.closest('.task');if(t)openTask(t);}});

/* ---------- cápsula / estados ---------- */
// Estados: normal ("idle"), dado velho ("stale", decidido pelo servidor), bom-dia ("morning", uma vez por sessão).
// Dado velho tem prioridade no estilo; se coincidir com o bom-dia, a saudação aparece junto da faixa.
var screen=document.getElementById('screen'),hud=document.getElementById('hud'),capBtn=document.getElementById('cap');
var pinned=false,hover=false,clicked=false,morning=false,forced=null,mt=null;
var canHover=window.matchMedia&&window.matchMedia('(hover:hover) and (pointer:fine)').matches;
// Na janela da casca (painel.html, <html data-janela>) a janela tem o tamanho do que está visível,
// porque a área transparente não deixa o clique passar. Abrir: aumenta a janela e depois anima.
// Recolher: anima e só no fim da transição encolhe, senão o cartão é cortado no meio.
var janela=document.documentElement.hasAttribute('data-janela'),aberto=false,seq=0;
var FOLGA={lado:24,topo:12,base:36}; // espaço para a sombra do vidro (--glass-shadow)
var CARTAO={largura:316,altura:470,vao:44}; // .card: width, height e bottom em painel.css
function medida(o){
  var c=capBtn.getBoundingClientRect();
  var w=o?Math.max(CARTAO.largura,c.width):c.width,h=o?CARTAO.altura+CARTAO.vao:c.height;
  return {largura:Math.ceil(w+2*FOLGA.lado),altura:Math.ceil(h+FOLGA.topo+FOLGA.base)};
}
function falhaJanela(e){console.warn('painel_tamanho',e);}
function depoisDaTransicao(el,fn){
  var feito=false,t=setTimeout(fim,350);
  function fim(){if(feito)return;feito=true;clearTimeout(t);el.removeEventListener('transitionend',fim);fn();}
  el.addEventListener('transitionend',fim);
}
/* ---------- direção do cartão (a cápsula pode estar em qualquer lugar da tela) ----------
   A cápsula mora no canto da janela que a casca mantém parado (ancora), com a mesma folga aberta ou
   recolhida; por isso ela não se mexe quando o cartão abre. Cápsula na metade de cima da área útil:
   o cartão abre para baixo; na metade esquerda: para a direita. A direção só muda com o cartão fechado. */
var pos=null,dir={v:'cima',h:'esquerda'};
function direcao(){
  if(!pos){ // fora da casca: padrão, ou simulado por ?abre=baixo-direita
    var a=(new URLSearchParams(location.search).get('abre')||'').split('-');
    return {v:a[0]==='baixo'?'baixo':'cima',h:a[1]==='direita'?'direita':'esquerda'};
  }
  // centro da cápsula, a partir do canto da janela onde ela está agora
  var cw=capBtn.offsetWidth,ch=capBtn.offsetHeight;
  var cx=dir.h==='direita'?pos.x+FOLGA.lado+cw/2:pos.x+pos.largura-FOLGA.lado-cw/2;
  var cy=dir.v==='baixo'?pos.y+FOLGA.topo+ch/2:pos.y+pos.altura-FOLGA.base-ch/2;
  return {v:cy<pos.area.altura/2?'baixo':'cima',h:cx<pos.area.largura/2?'direita':'esquerda'};
}
function aplicarDirecao(){
  dir=direcao();
  if(dir.v==='baixo')hud.setAttribute('data-v','baixo');else hud.removeAttribute('data-v');
  if(dir.h==='direita')hud.setAttribute('data-h','direita');else hud.removeAttribute('data-h');
}
function ancora(){return (dir.v==='baixo'?'superior':'inferior')+'-'+(dir.h==='direita'?'esquerdo':'direito');}
function tamanho(o){var m=medida(o);m.ancora=ancora();return ajustarJanela(m);}

// 'aberto' conta só a passagem de fechado para aberto por ação do usuário (hover ou clique); bom-dia não conta.
var visivel=false;
function paint(){
  var o=pinned||hover||clicked;capBtn.setAttribute('aria-expanded',o?'true':'false');
  if(o&&!visivel&&!pinned)avisarEvento('aberto');
  visivel=o;
  if(!janela){if(o&&!hud.classList.contains('open'))aplicarDirecao();hud.classList.toggle('open',o);return;}
  if(o===aberto)return;aberto=o;var n=++seq;
  if(o){aplicarDirecao();tamanho(true).catch(falhaJanela).then(function(){if(n===seq)hud.classList.add('open');});}
  else{hud.classList.remove('open');depoisDaTransicao(document.getElementById('card'),function(){if(n===seq)tamanho(false).catch(falhaJanela);});}
}
// a cápsula muda de largura com os números; recolhida, a janela acompanha
function remedir(){if(janela&&!aberto&&!arrastando)tamanho(false).catch(falhaJanela);}

/* ---------- arraste da cápsula ----------
   Clique curto abre o cartão; passou de LIMIAR px com o botão pressionado, vira arraste nativo (casca).
   Antes de arrastar, o cartão recolhe sem animação e a janela volta ao tamanho da cápsula, para ela não
   mudar de tamanho durante o arraste. O fim é o evento 'painel://posicao' (no Windows o pointerup
   provavelmente não chega) ou o pointerup, se vier. O clique que sobra de um arraste é engolido. */
var LIMIAR=5,toque=null,arrastando=false,engolirClique=false;
function iniciarArraste(){
  arrastando=true;engolirClique=true;visivel=false; // o cartão fecha aqui sem passar pelo paint()
  clearTimeout(mt);morning=false;pinned=false;clicked=false;hover=false;applyMode();
  capBtn.setAttribute('aria-expanded','false');
  var antes=Promise.resolve();
  if(hud.classList.contains('open')||aberto){
    seq++;aberto=false;
    hud.classList.add('sem-anim');hud.classList.remove('open');void hud.offsetWidth;hud.classList.remove('sem-anim');
    if(janela)antes=tamanho(false);
  }
  antes.then(arrastarJanela).catch(function(e){console.warn('painel_arrastar',e);fimArraste();});
}
function fimArraste(){arrastando=false;toque=null;}
capBtn.addEventListener('pointerdown',function(e){if(e.button!==0)return;toque={x:e.screenX,y:e.screenY};engolirClique=false;});
capBtn.addEventListener('pointermove',function(e){
  if(!toque||arrastando||!(e.buttons&1))return;
  if(Math.hypot(e.screenX-toque.x,e.screenY-toque.y)>=LIMIAR)iniciarArraste();
});
document.addEventListener('pointerup',function(){toque=null;if(arrastando)fimArraste();});
aoMudarPosicao(function(p){pos=p;if(arrastando)fimArraste();});
if(janela)posicaoJanela().then(function(p){if(p)pos=p;},function(e){console.warn('painel_posicao',e);});
if(janela&&document.fonts)document.fonts.ready.then(remedir); // a fonte muda a largura da cápsula
function applyMode(){
  var m=forced||(stale?'stale':morning?'morning':'idle');
  screen.setAttribute('data-mode',m);
  screen.toggleAttribute('data-bomdia',m==='stale'&&morning); // dado velho no bom-dia: saudação + faixa âmbar
  remedir();
  var bs=document.querySelectorAll('.seg button');for(var i=0;i<bs.length;i++)bs[i].setAttribute('aria-pressed',bs[i].getAttribute('data-mode')===m?'true':'false');
}
function goodMorning(){
  avisarEvento('bom_dia');
  morning=true;pinned=true;clicked=false;hover=false;showTab('hoje');applyMode();paint();
  var d=document.querySelector('.auto .drain i');if(d){d.style.animation='none';void d.offsetWidth;d.style.animation='';}
  clearTimeout(mt);mt=setTimeout(endMorning,(cfg.morning.duration_seconds||10)*1000);
}
function endMorning(){clearTimeout(mt);if(!morning)return;morning=false;pinned=false;applyMode();paint();}
// seletor "Situação simulada" do protótipo: força o estado para revisão visual
var seg=document.querySelector('.seg');
if(seg)seg.addEventListener('click',function(e){
  var b=e.target.closest('button');if(!b)return;var m=b.getAttribute('data-mode');
  endMorning();forced=(m==='idle')?null:m;
  if(m==='morning'){forced=null;goodMorning();}else{applyMode();paint();}
});
capBtn.addEventListener('click',function(){if(engolirClique){engolirClique=false;return;}if(pinned){endMorning();}else{clicked=!clicked;}paint();});
// com o botão pressionado pode ser arraste: o hover não abre
if(canHover){hud.addEventListener('mouseenter',function(){if(arrastando||toque)return;hover=true;paint();});hud.addEventListener('mouseleave',function(){if(arrastando)return;hover=false;paint();});}
document.addEventListener('keydown',function(e){if(e.key==='Escape'){clicked=false;endMorning();paint();}});

/* ---------- início ---------- */
// Bom-dia: uma vez ao abrir o agente (= início da sessão do Windows, via autostart),
// respeitando me/config: enabled, once_per_day, not_before.
function morningDue(){
  var m=cfg.morning;if(!m||!m.enabled||!data)return false;
  var now=new Date();if(m.not_before&&hhmm(now)<m.not_before)return false;
  if(m.once_per_day){try{if(localStorage.getItem('bomdia')===data.today)return false;localStorage.setItem('bomdia',data.today);}catch(e){}}
  return true;
}
var timer=null;
function schedule(){clearInterval(timer);timer=setInterval(load,Math.max(60,cfg.poll_interval_seconds||600)*1000);}
api.getConfig().then(function(c){if(c)cfg=c;},function(err){console.warn('gestor config',err&&err.code);})
  .then(load).then(function(){schedule();if(morningDue())goodMorning();});
paint();
