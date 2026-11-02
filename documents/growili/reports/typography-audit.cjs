const {chromium}=require('/usr/lib/node_modules/openclaw/node_modules/playwright-core');
(async()=>{
 const b=await chromium.launch({headless:true,args:['--no-sandbox']});
 for(const width of [1440,390,320]){
  const p=await b.newPage({viewport:{width,height:900},locale:'cs-CZ'});
  await p.goto('https://growili.com',{waitUntil:'networkidle'});
  const cz=p.getByRole('button',{name:'Switch to Czech'});
  if(await cz.count()) await cz.click();
  await p.evaluate(()=>document.fonts.ready);
  for(let y=0;y<await p.evaluate(()=>document.body.scrollHeight);y+=700){await p.evaluate(y=>scrollTo(0,y),y);await p.waitForTimeout(80)}
  await p.evaluate(()=>scrollTo(0,0));await p.waitForTimeout(1000);
  console.log(JSON.stringify({width,...await p.evaluate(()=>{
   const els=[...document.querySelectorAll('h1,h2,h3,p,button,a')].filter(e=>e.getBoundingClientRect().width&&e.getBoundingClientRect().height);
   const data=e=>{const c=getComputedStyle(e),r=e.getBoundingClientRect();return {tag:e.tagName,text:e.innerText?.trim().slice(0,130),font:c.fontFamily,size:c.fontSize,weight:c.fontWeight,line:c.lineHeight,spacing:c.letterSpacing,color:c.color,bg:c.backgroundColor,x:Math.round(r.x),y:Math.round(r.y),w:Math.round(r.width),h:Math.round(r.height),scroll:e.scrollWidth,client:e.clientWidth}};
   return {url:location.href,lang:document.documentElement.lang,fonts:[...document.fonts].map(f=>({family:f.family,status:f.status,weight:f.weight})),pageWidth:document.documentElement.scrollWidth,headings:els.filter(e=>/^H/.test(e.tagName)).map(data),paragraphs:els.filter(e=>e.tagName==='P').map(data),buttons:els.filter(e=>e.tagName==='BUTTON').map(data),overflow:[...document.querySelectorAll('body *')].filter(e=>{const r=e.getBoundingClientRect();return r.width>0&&(r.right>innerWidth+1||r.left< -1)&&getComputedStyle(e).position!=='fixed'}).slice(0,15).map(data)};
  })}));
  await p.screenshot({path:`/home/growthhacker/.openclaw/workspaces/coo/documents/growili/reports/homepage-${width}.png`,fullPage:true});
  await p.close();
 }
 await b.close();
})().catch(e=>{console.error(e);process.exit(1)});
