include GAstUtils
open Types
open LlbcAst
open Utils

(** Check if a {!type:Charon.LlbcAst.statement} contains loops *)
let statement_has_loops (st : statement) : bool =
  let obj =
    object
      inherit [_] iter_statement
      method! visit_Loop _ _ = raise Found
    end
  in
  try
    obj#visit_statement () st;
    false
  with Found -> true

(** Check if a {!type:Charon.LlbcAst.fun_decl} contains loops *)
let fun_decl_has_loops (fd : fun_decl) : bool =
  match fd.body with
  | Some body -> statement_has_loops body.body
  | None -> false

let compute_defs_maps (c : crate) :
    type_decl TypeDeclId.Map.t
    * fun_decl FunDeclId.Map.t
    * global_decl GlobalDeclId.Map.t =
  GAstUtils.compute_defs_maps (fun g -> g.def_id) c

(** Create a sequence *)
let mk_sequence (st1 : statement) (st2 : statement) : statement =
  let meta = MetaUtils.combine_meta st1.meta st2.meta in
  let content = Sequence (st1, st2) in
  { meta; content }

(** Chain two statements into a sequence, by pushing the second statement
    at the end of the first one (diving into sequences, switches, etc.).
 *)
let rec chain_statements (st1 : statement) (st2 : statement) : statement =
  match st1.content with
  | SetDiscriminant _ | Assert _ | Call _ | Assign _ | FakeRead _ | Drop _
  | Loop _ ->
      (* Simply create a sequence *)
      mk_sequence st1 st2
  | Nop -> (* Ignore the nop *) st2
  | Break _ | Continue _ | Panic | Return ->
      (* Ignore the second statement, which won't be evaluated *) st1
  | Switch switch ->
      (* Insert inside the switch *)
      let meta = MetaUtils.combine_meta st1.meta st2.meta in
      let content = Switch (chain_statements_in_switch switch st2) in
      { meta; content }
  | Sequence (st3, st4) ->
      (* Insert at the end of the statement *)
      mk_sequence st3 (chain_statements st4 st2)

and chain_statements_in_switch (switch : switch) (st : statement) : switch =
  match switch with
  | If (op, st0, st1) ->
      If (op, chain_statements st0 st, chain_statements st1 st)
  | SwitchInt (op, int_ty, branches, otherwise) ->
      let branches =
        List.map (fun (svl, br) -> (svl, chain_statements br st)) branches
      in
      let otherwise = chain_statements otherwise st in
      SwitchInt (op, int_ty, branches, otherwise)
  | Match (op, branches, otherwise) ->
      let branches =
        List.map (fun (svl, br) -> (svl, chain_statements br st)) branches
      in
      let otherwise = chain_statements otherwise st in
      Match (op, branches, otherwise)
