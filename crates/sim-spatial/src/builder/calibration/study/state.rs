//! Retained offline authoring state. Only actions and job-result publication write it.
use crate::document::{DocumentId, DocumentRegistry};
use crate::app::ViewerMode;
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use sim_runtime::experiment_study::Study;
use super::jobs::{PendingJob, JobReceipt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudyStamp { pub id: u64, pub revision: u64 }

/// Shared document identity, not a second document or spatial-selection service.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentCapture {
    pub id: DocumentId,
    pub revision: u64,
    pub source: crate::document::Source,
}
impl DocumentCapture {
    pub fn current(registry: &DocumentRegistry) -> Option<Self> {
        registry.entry(ViewerMode::Build).map(|e| Self { id:e.id, revision:e.revision, source:e.source.clone() })
    }
    pub fn matches(&self, registry: &DocumentRegistry) -> bool {
        Self::current(registry).as_ref() == Some(self)
    }
}

pub struct RetainedStudy {
    pub id: u64,
    pub revision: u64,
    pub saved_revision: Option<u64>,
    pub study: Study,
    pub document: Option<DocumentCapture>,
    pub source: String,
    /// Unexpected document displacement keeps evidence here, visibly labelled.
    pub displaced: Option<String>,
}
impl RetainedStudy {
    pub fn stamp(&self) -> StudyStamp { StudyStamp { id:self.id, revision:self.revision } }
    pub fn dirty(&self) -> bool { self.saved_revision != Some(self.revision) }
}

#[derive(Resource, Default)]
pub struct StudyOwner {
    pub studies: Vec<RetainedStudy>,
    pub active: Option<u64>,
    pub pending: Vec<PendingJob>,
    pub receipts: Vec<JobReceipt>,
    pub status: String,
    pub changed: u64,
    pub(crate) next_study: u64,
    pub(crate) next_job: u64,
    /// Changes on explicit navigation as well as loading: late loads never steal focus.
    pub(crate) selection_epoch: u64,
}
impl StudyOwner {
    pub fn active(&self) -> Option<&RetainedStudy> { self.studies.iter().find(|s|Some(s.id)==self.active) }
    pub fn get(&self, id:u64) -> Option<&RetainedStudy> { self.studies.iter().find(|s|s.id==id) }
    pub(crate) fn get_mut(&mut self,id:u64)->Option<&mut RetainedStudy> { self.studies.iter_mut().find(|s|s.id==id) }
    pub fn busy(&self)->bool { !self.pending.is_empty() }
    pub fn blocking_reason(&self)->Option<String> {
        if !self.pending.is_empty() { return Some("Measured study work is pending. Cancel or wait and save the retained evidence before leaving.".into()); }
        self.studies.iter().find(|s|s.dirty()).map(|s|format!("Measured study {} has unsaved draft or evidence. Save a new review before leaving.",s.id))
    }
    pub fn validate_stamp(&self, stamp:StudyStamp)->Result<&RetainedStudy,String> {
        let s=self.get(stamp.id).ok_or("study.id: retained study is missing")?;
        if self.active!=Some(stamp.id) { return Err("study.id: the displayed study was replaced; retained inputs were not applied".into()); }
        if s.revision!=stamp.revision { return Err("study.revision: the displayed draft changed; refresh the field before submitting".into()); }
        Ok(s)
    }
    pub(crate) fn retain(&mut self, study:Study, source:String, document:Option<DocumentCapture>, saved:bool, activate:bool)->u64 {
        self.next_study+=1;
        let id=self.next_study;
        self.studies.push(RetainedStudy {id,revision:0,saved_revision:saved.then_some(0),study,document,source,displaced:None});
        if activate { self.active=Some(id); self.selection_epoch+=1; }
        self.changed+=1;
        id
    }
    pub fn snapshot(&self)->serde_json::Value {
        serde_json::json!({"active":self.active,"status":self.status,"studies":self.studies.iter().map(|s|serde_json::json!({"id":s.id,"revision":s.revision,"saved_revision":s.saved_revision,"dirty":s.dirty(),"source":s.source,"displaced":s.displaced,"study":s.study})).collect::<Vec<_>>(),"jobs":self.pending.iter().map(|p|p.snapshot()).collect::<Vec<_>>(),"receipts":self.receipts.iter().map(|r|r.snapshot()).collect::<Vec<_>>()})
    }
}
