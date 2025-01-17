#![allow(dead_code)]
use crate::meta::{FileId, FileInfo, FileName, LocalFileId, VirtualFileId};
use crate::reorder_decls as rd;
use crate::types as ty;
use crate::types::TypeDeclId;
use crate::ullbc_ast as ast;
use crate::ullbc_ast::FunDeclId;
use crate::ullbc_ast::GlobalDeclId;
use rustc_hir::def_id::DefId;
use std::collections::HashMap;
use std::vec::Vec;

pub type GDeclarationGroup<Id> = rd::GDeclarationGroup<Id>;
pub type TypeDeclarationGroup = rd::GDeclarationGroup<ty::TypeDeclId::Id>;
pub type FunDeclarationGroup = rd::GDeclarationGroup<ast::FunDeclId::Id>;
pub type DeclarationGroup =
    rd::DeclarationGroup<ty::TypeDeclId::Id, ast::FunDeclId::Id, ast::GlobalDeclId::Id>;

pub type AnyDeclRid = rd::AnyDeclId<DefId, DefId, DefId>;
pub type AnyDeclId = rd::AnyDeclId<ty::TypeDeclId::Id, ast::FunDeclId::Id, ast::GlobalDeclId::Id>;

#[derive(Clone, Copy)]
/// Information common to any top-level declaration.
pub struct DeclInfo {
    /// Its Rust identifier. Indicates if the declaration is local ("external" otherwise).
    pub rid: DefId,
    /// True if the declaration's body is accessible ("opaque" otherwise).
    pub is_transparent: bool,
}
impl DeclInfo {
    fn new(rid: DefId, info: rd::DeclInfo) -> Self {
        DeclInfo {
            rid,
            is_transparent: info.is_transparent,
        }
    }
    pub fn is_local(&self) -> bool {
        self.rid.is_local()
    }
}

// Small helpers.
fn add_type_info(
    src: &HashMap<AnyDeclRid, rd::DeclInfo>,
    dst: &mut HashMap<AnyDeclId, DeclInfo>,
    rid: DefId,
    id: TypeDeclId::Id,
) {
    let info = *src.get(&AnyDeclRid::Type(rid)).unwrap();
    dst.insert(AnyDeclId::Type(id), DeclInfo::new(rid, info));
}
fn add_function_info(
    src: &HashMap<AnyDeclRid, rd::DeclInfo>,
    dst: &mut HashMap<AnyDeclId, DeclInfo>,
    rid: DefId,
    id: FunDeclId::Id,
) {
    let info = *src.get(&AnyDeclRid::Fun(rid)).unwrap();
    dst.insert(AnyDeclId::Fun(id), DeclInfo::new(rid, info));
}
fn add_global_info(
    src: &HashMap<AnyDeclRid, rd::DeclInfo>,
    dst: &mut HashMap<AnyDeclId, DeclInfo>,
    rid: DefId,
    id: GlobalDeclId::Id,
) {
    let info = *src.get(&AnyDeclRid::Global(rid)).unwrap();
    dst.insert(AnyDeclId::Global(id), DeclInfo::new(rid, info));
}

pub struct OrderedDecls {
    /// The ordered files
    pub files: Vec<FileName>,
    /// The properly grouped and ordered declarations
    pub decls: Vec<DeclarationGroup>,
    /// Additional information on declarations
    pub decls_info: HashMap<AnyDeclId, DeclInfo>,
    /// File names to ids and vice-versa
    pub file_to_id: HashMap<FileName, FileId::Id>,
    pub id_to_file: HashMap<FileId::Id, FileName>,
    /// Rust identifiers to translation identifiers
    pub type_rid_to_id: HashMap<DefId, ty::TypeDeclId::Id>,
    pub fun_rid_to_id: HashMap<DefId, ast::FunDeclId::Id>,
    pub global_rid_to_id: HashMap<DefId, ast::GlobalDeclId::Id>,
}

/// Convert the definition ids used by the rust compiler to our own definition ids.
pub fn rust_to_local_ids(
    files_info: &HashMap<FileName, FileInfo>,
    reordered: &rd::DeclarationsGroups<DefId, DefId, DefId>,
) -> OrderedDecls {
    let mut decls_info = HashMap::new();

    let mut type_rid_to_id: HashMap<DefId, ty::TypeDeclId::Id> = HashMap::new();
    let mut fun_rid_to_id: HashMap<DefId, ast::FunDeclId::Id> = HashMap::new();
    let mut global_rid_to_id: HashMap<DefId, ast::GlobalDeclId::Id> = HashMap::new();

    let mut type_counter = ty::TypeDeclId::Generator::new();
    let mut fun_counter = ast::FunDeclId::Generator::new();
    let mut global_counter = ast::GlobalDeclId::Generator::new();

    let mut decls: Vec<DeclarationGroup> = Vec::new();

    // Compute the translated list of declarations and the mappings from rust
    // identifiers to translation identifiers and vice-versa.
    for decl in &reordered.decls {
        match decl {
            rd::DeclarationGroup::Type(rd::GDeclarationGroup::NonRec(rid)) => {
                let id = type_counter.fresh_id();
                type_rid_to_id.insert(*rid, id);
                add_type_info(&reordered.decls_info, &mut decls_info, *rid, id);
                decls.push(DeclarationGroup::Type(GDeclarationGroup::NonRec(id)));
            }
            rd::DeclarationGroup::Type(rd::GDeclarationGroup::Rec(rids)) => {
                let mut ids: Vec<ty::TypeDeclId::Id> = Vec::new();
                for rid in rids {
                    let id = type_counter.fresh_id();
                    type_rid_to_id.insert(*rid, id);
                    add_type_info(&reordered.decls_info, &mut decls_info, *rid, id);
                    ids.push(id);
                }
                decls.push(DeclarationGroup::Type(GDeclarationGroup::Rec(ids)));
            }
            rd::DeclarationGroup::Fun(rd::GDeclarationGroup::NonRec(rid)) => {
                let id = fun_counter.fresh_id();
                fun_rid_to_id.insert(*rid, id);
                add_function_info(&reordered.decls_info, &mut decls_info, *rid, id);
                decls.push(DeclarationGroup::Fun(GDeclarationGroup::NonRec(id)));
            }
            rd::DeclarationGroup::Fun(rd::GDeclarationGroup::Rec(rids)) => {
                let mut ids: Vec<ast::FunDeclId::Id> = Vec::new();
                for rid in rids {
                    let id = fun_counter.fresh_id();
                    fun_rid_to_id.insert(*rid, id);
                    add_function_info(&reordered.decls_info, &mut decls_info, *rid, id);
                    ids.push(id);
                }
                decls.push(DeclarationGroup::Fun(GDeclarationGroup::Rec(ids)));
            }
            rd::DeclarationGroup::Global(rd::GDeclarationGroup::NonRec(rid)) => {
                let id = global_counter.fresh_id();
                global_rid_to_id.insert(*rid, id);
                add_global_info(&reordered.decls_info, &mut decls_info, *rid, id);
                decls.push(DeclarationGroup::Global(GDeclarationGroup::NonRec(id)));
            }
            rd::DeclarationGroup::Global(rd::GDeclarationGroup::Rec(rids)) => {
                let mut ids: Vec<ast::GlobalDeclId::Id> = Vec::new();
                for rid in rids {
                    let id = global_counter.fresh_id();
                    global_rid_to_id.insert(*rid, id);
                    add_global_info(&reordered.decls_info, &mut decls_info, *rid, id);
                    ids.push(id);
                }
                decls.push(DeclarationGroup::Global(GDeclarationGroup::Rec(ids)));
            }
        }
    }

    // Reorder the files and compute the maps from files to ids and reverse
    let mut files: Vec<FileName> = files_info.keys().cloned().collect();
    files.sort();

    let mut file_to_id: HashMap<FileName, FileId::Id> = HashMap::new();
    let mut id_to_file: HashMap<FileId::Id, FileName> = HashMap::new();
    let mut real_file_counter = LocalFileId::Generator::new();
    let mut virtual_file_counter = VirtualFileId::Generator::new();

    for file in &files {
        let id = match file {
            FileName::Local(_) => FileId::Id::LocalId(real_file_counter.fresh_id()),
            FileName::Virtual(_) => FileId::Id::VirtualId(virtual_file_counter.fresh_id()),
            FileName::NotReal(_) => unimplemented!(),
        };
        file_to_id.insert(file.clone(), id);
        id_to_file.insert(id, file.clone());
    }

    OrderedDecls {
        files,
        decls,
        decls_info,
        file_to_id,
        id_to_file,
        type_rid_to_id,
        fun_rid_to_id,
        global_rid_to_id,
    }
}
