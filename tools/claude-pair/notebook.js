let notebookRendered=null;
function renderNotebook(){
 const book=data?.notebook;if(!book)return;
 $('notebook-count').textContent=book.total+' entries';
 const feed=$('notebook-feed'),atEnd=feed.scrollHeight-feed.scrollTop-feed.clientHeight<35;
 const html=book.entries.length?book.entries.map(e=>`<article class="notebook-entry"><div class="notebook-entry-head"><span class="notebook-author ${escapeHTML(e.author)}">${escapeHTML(pretty(e.author))}</span><span>${String(e.sequence).padStart(3,'0')} · ${escapeHTML(e.kind)}${e.historical?' · imported history':''}</span><time>${escapeHTML(new Date(e.at*1000).toLocaleString())}</time></div><p>${escapeHTML(e.summary)}</p>${e.notes?.length?'<ul>'+e.notes.map(n=>'<li>'+escapeHTML(n)+'</li>').join('')+'</ul>':''}<details><summary>Entry ID &amp; source</summary><div class="notebook-source">${escapeHTML(e.id)}<br>${escapeHTML(e.source||'')}</div></details></article>`).join(''):'<div class="empty">The shared notebook will be initialized before the next agent turn.</div>';
 if(html!==notebookRendered){const first=notebookRendered===null;const scroll=feed.scrollTop;feed.innerHTML=html;notebookRendered=html;feed.scrollTop=first||atEnd?feed.scrollHeight:scroll}
 $('notebook-caption').textContent=(book.total>book.entries.length?'Showing the latest '+book.entries.length+' entries, oldest first. ':'Oldest first. ')+ 'Reports and proposals still require verification; unanswered questions remain open.';
}
function initNotebook(){
 $('notebook-system').onclick=()=>modal('The shared system guide',data?.notebook?.system||'Loading…');
 $('notebook-current').onclick=()=>modal('Current context shared with the team',JSON.stringify({status:data.state.status,phase:data.state.phase,objective:data.mission,batch:data.state.outer?.current_batch,assignment:data.state.plan,latest_worker_report:data.state.report,checks:data.state.receipts,user_guidance:data.steering?.text,notebook:data.notebook.path},null,2));
 $('notebook-full').onclick=async()=>{try{const r=await fetch('/api/journal');if(!r.ok)throw Error('Could not read the journal');modal('Full shared journal · chronological',await r.text())}catch(e){toast(e.message)}};
 $('notebook-latest').onclick=()=>{const feed=$('notebook-feed');feed.scrollTop=feed.scrollHeight};
}
