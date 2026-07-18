//! GraphQLクエリドキュメントの検証(validation)層。
//!
//! `parser`はGraphQL仕様(2021年10月版 §2 Language)の文法に従って
//! `Document`を構築するだけで、意味的な妥当性(仕様 §5 Validation)は
//! 一切チェックしない。本モジュールはパース済み`Document`を受け取り、
//! 仕様§5のうち「スキーマ(型システム)を必要としない、ドキュメント自体
//! だけで判定できるルール」の部分集合を実装する。
//!
//! ## スキーマ非依存という前提について
//!
//! 実装前に`ast.rs`/`lib.rs`全体を確認したが、このクレートには
//! スキーマ定義言語(SDL)や型システムの表現が一切存在しない
//! (`CLAUDE.md`の「未着手」にも明記の通りSDLパーサーは次段階)。
//! そのため「フィールドが型に存在するか」「引数の型が一致するか」等の
//! 型システム依存ルールは本モジュールの範囲外とし、ドキュメント構造
//! だけで判定できる構文的・構造的ルールのみを対象とする。
//!
//! ## 実装したルール
//!
//! - **5.5.1.1 Fragment Spread Target Defined**(`UndefinedFragment`):
//!   `...Name`が参照する`Name`は文書内の`fragment Name on ...`として
//!   定義されていなければならない。
//! - **5.5.2.2 Fragments Must Not Form Cycles**(`FragmentCycle`):
//!   フラグメントが直接または間接的に自分自身をスプレッドしてはならない。
//! - **5.5.1.4 Fragments Must Be Used**(`UnusedFragment`):
//!   定義されたフラグメントは、いずれかの操作から直接/間接的に
//!   到達可能でなければならない(未使用フラグメント定義はエラー)。
//! - **5.2.2.1 Lone Anonymous Operation**(`LoneAnonymousOperation`):
//!   無名操作(`{ ... }`短縮形)は、文書内に他の操作が無い場合に限り許可。
//! - **5.2.1.1 Operation Name Uniqueness**(`DuplicateOperationName`):
//!   名前付き操作の名前は文書内で重複してはならない。
//! - **5.8.3 Variables Are Used**(`UnusedVariable`)/
//!   **5.8.4 All Variable Uses Defined**(`UndefinedVariable`):
//!   操作で宣言された変数(`$name`)は、その操作の選択集合内
//!   (スプレッドしたフラグメントを含め再帰的に)で最低1回使用されなければ
//!   ならず、逆に選択集合内で使用される変数は必ずその操作で宣言されて
//!   いなければならない。
//!   **注意**: `UnusedVariable`(未使用変数エラー)は、実装によっては
//!   緩い(warning止まり)場合がある。仕様書 §5.8.3
//!   "All variables defined by an operation must be used in that operation
//!   or a fragment transitively included by that operation" は明確に
//!   MUSTとして記載されているため、本実装ではエラーとして扱うが、
//!   将来的に選択的に無効化できるよう、他の(疑いの余地がない)ルールとは
//!   別の`ValidationErrorKind`として区別してある。
//! - **5.3.2 Field Selection Merging**(`ConflictingFieldSelection`)の
//!   **部分集合**: 同一の選択集合(`SelectionSet`)直下で、同じ
//!   レスポンスキー(エイリアス、無ければフィールド名)を持つ複数の
//!   `Field`が、異なるフィールド名または異なる引数を持つ場合に衝突として
//!   報告する。
//!   **実装した範囲 vs 仕様の全体像(正直な開示)**: 仕様の本ルールは
//!   本来、フラグメントスプレッドやインラインフラグメントを展開した後の
//!   「実際にマージされる選択集合」全体・かつ再帰的なサブ選択集合の
//!   マージ可能性まで含めて判定する非常に一般的なアルゴリズムである。
//!   本実装は **同一`SelectionSet`直下の直接の`Field`同士のみ** を比較し、
//!   フラグメントスプレッド越しに同じレスポンスキーが衝突するケース
//!   (例: `...FragA`と`...FragB`がそれぞれ同じエイリアスで矛盾する
//!   フィールドを提供する場合)は検出しない。これは意図的な簡略化であり、
//!   将来的な拡張点として明記しておく。

use std::collections::{HashMap, HashSet};

use crate::ast::{
    Directive, Document, Field, FragmentDefinition, OperationDefinition, Selection, SelectionSet,
    Value,
};

/// 検証エラーの種別。ルールごとに区別し、将来的に選択的無効化や
/// メッセージの機械的分類ができるようにしてある。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorKind {
    /// 5.5.1.1 Fragment Spread Target Defined
    UndefinedFragment,
    /// 5.5.2.2 Fragments Must Not Form Cycles
    FragmentCycle,
    /// 5.5.1.4 Fragments Must Be Used
    UnusedFragment,
    /// 5.2.2.1 Lone Anonymous Operation
    LoneAnonymousOperation,
    /// 5.2.1.1 Operation Name Uniqueness
    DuplicateOperationName,
    /// 5.8.4 All Variable Uses Defined
    UndefinedVariable,
    /// 5.8.3 Variables Are Used(実装によっては緩められる余地がある
    /// ルールなので、他と区別できるよう独立したvariantにしてある)。
    UnusedVariable,
    /// 5.3.2 Field Selection Mergingの部分集合
    ConflictingFieldSelection,
}

/// 検証エラー1件。どのルールに違反したか(`kind`)と、人間可読な
/// メッセージ(`message`)を持つ。
///
/// **位置情報についての注意**: このクレートのAST(`ast.rs`)は
/// トークンの`start`オフセットを保持しない(`parser.rs`が`Token`から
/// `Document`を構築する際に位置情報を捨てている)。そのためソース位置は
/// 付与できず、メッセージ内に操作名・フラグメント名・変数名などの
/// 識別情報を含めることで代替している。位置情報を持たせたい場合は
/// AST自体に`span`フィールドを追加する必要があり、これは本タスクの
/// スコープ外(既存AST形状の変更は最小限に留める方針)とした。
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// `Document`を検証し、見つかった検証エラーをすべて返す
/// (最初の1件で打ち切らない — 実際のGraphQLツールと同じく複数の
/// 独立した検証エラーを同時に報告する)。空のベクタは「検証を通過した」
/// ことを意味する。
pub fn validate(document: &Document) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let fragments: HashMap<&str, &FragmentDefinition> = document
        .fragments
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    validate_known_fragment_names(document, &fragments, &mut errors);
    validate_fragment_cycles(document, &mut errors);
    validate_unused_fragments(document, &fragments, &mut errors);
    validate_operations(document, &mut errors);
    validate_variables(document, &fragments, &mut errors);
    validate_field_selection_merging(document, &mut errors);

    errors
}

/// `set`直下および再帰的に到達可能な、すべての`Selection`を平坦化して
/// 返す(フラグメントスプレッドの参照先までは展開しない — スプレッド
/// 自体は`Selection::FragmentSpread`として1件収集される)。
fn all_selections(set: &SelectionSet) -> Vec<&Selection> {
    fn rec<'a>(set: &'a SelectionSet, out: &mut Vec<&'a Selection>) {
        for sel in &set.selections {
            out.push(sel);
            match sel {
                Selection::Field(f) => {
                    if let Some(sub) = &f.selection_set {
                        rec(sub, out);
                    }
                }
                Selection::InlineFragment(inline) => rec(&inline.selection_set, out),
                Selection::FragmentSpread(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    rec(set, &mut out);
    out
}

fn operation_label(op: &OperationDefinition) -> String {
    match &op.name {
        Some(name) => format!("操作 `{}`", name),
        None => "無名操作".to_string(),
    }
}

// --- 5.5.1.1 Fragment Spread Target Defined ---

fn validate_known_fragment_names(
    document: &Document,
    fragments: &HashMap<&str, &FragmentDefinition>,
    errors: &mut Vec<ValidationError>,
) {
    let mut check = |set: &SelectionSet, context: &str| {
        for sel in all_selections(set) {
            if let Selection::FragmentSpread(spread) = sel {
                if !fragments.contains_key(spread.name.as_str()) {
                    errors.push(ValidationError {
                        kind: ValidationErrorKind::UndefinedFragment,
                        message: format!(
                            "{}が参照するフラグメント `...{}` は定義されていません",
                            context, spread.name
                        ),
                    });
                }
            }
        }
    };

    for op in &document.operations {
        check(&op.selection_set, &operation_label(op));
    }
    for frag in &document.fragments {
        check(
            &frag.selection_set,
            &format!("フラグメント定義 `{}`", frag.name),
        );
    }
}

// --- 5.5.2.2 Fragments Must Not Form Cycles ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

fn validate_fragment_cycles(document: &Document, errors: &mut Vec<ValidationError>) {
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for frag in &document.fragments {
        let names = all_selections(&frag.selection_set)
            .into_iter()
            .filter_map(|sel| match sel {
                Selection::FragmentSpread(spread) => Some(spread.name.as_str()),
                _ => None,
            })
            .collect();
        deps.insert(frag.name.as_str(), names);
    }

    let mut color: HashMap<&str, Color> = deps.keys().map(|&k| (k, Color::White)).collect();
    let names: Vec<&str> = document.fragments.iter().map(|f| f.name.as_str()).collect();

    for start in names {
        if color.get(start) == Some(&Color::White) {
            let mut stack = Vec::new();
            dfs_fragment_cycle(start, &deps, &mut color, &mut stack, errors);
        }
    }
}

fn dfs_fragment_cycle<'a>(
    node: &'a str,
    deps: &HashMap<&'a str, Vec<&'a str>>,
    color: &mut HashMap<&'a str, Color>,
    stack: &mut Vec<&'a str>,
    errors: &mut Vec<ValidationError>,
) {
    color.insert(node, Color::Gray);
    stack.push(node);
    if let Some(next_list) = deps.get(node) {
        for &next in next_list {
            match color.get(next) {
                Some(Color::Gray) => {
                    let idx = stack.iter().position(|&n| n == next).unwrap_or(0);
                    let cycle_path: Vec<&str> = stack[idx..].to_vec();
                    let mut path_desc = cycle_path.join(" -> ");
                    path_desc.push_str(&format!(" -> {}", next));
                    errors.push(ValidationError {
                        kind: ValidationErrorKind::FragmentCycle,
                        message: format!("フラグメントの循環参照を検出しました: {}", path_desc),
                    });
                }
                Some(Color::Black) => {}
                Some(Color::White) | None => {
                    if deps.contains_key(next) {
                        dfs_fragment_cycle(next, deps, color, stack, errors);
                    }
                }
            }
        }
    }
    stack.pop();
    color.insert(node, Color::Black);
}

// --- 5.5.1.4 Fragments Must Be Used ---

fn validate_unused_fragments(
    document: &Document,
    fragments: &HashMap<&str, &FragmentDefinition>,
    errors: &mut Vec<ValidationError>,
) {
    let mut reachable: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = Vec::new();

    for op in &document.operations {
        for sel in all_selections(&op.selection_set) {
            if let Selection::FragmentSpread(spread) = sel {
                if reachable.insert(spread.name.as_str()) {
                    queue.push(spread.name.as_str());
                }
            }
        }
    }

    while let Some(name) = queue.pop() {
        if let Some(frag) = fragments.get(name) {
            for sel in all_selections(&frag.selection_set) {
                if let Selection::FragmentSpread(spread) = sel {
                    if reachable.insert(spread.name.as_str()) {
                        queue.push(spread.name.as_str());
                    }
                }
            }
        }
    }

    for frag in &document.fragments {
        if !reachable.contains(frag.name.as_str()) {
            errors.push(ValidationError {
                kind: ValidationErrorKind::UnusedFragment,
                message: format!(
                    "フラグメント `{}` はどの操作からも参照されていません(未使用)",
                    frag.name
                ),
            });
        }
    }
}

// --- 5.2.2.1 Lone Anonymous Operation / 5.2.1.1 Operation Name Uniqueness ---

fn validate_operations(document: &Document, errors: &mut Vec<ValidationError>) {
    if document.operations.len() > 1 {
        for op in &document.operations {
            if op.name.is_none() {
                errors.push(ValidationError {
                    kind: ValidationErrorKind::LoneAnonymousOperation,
                    message:
                        "無名操作は、文書内に他の操作が存在しない場合に限り許可されます"
                            .to_string(),
                });
            }
        }
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    for op in &document.operations {
        if let Some(name) = &op.name {
            *seen.entry(name.as_str()).or_insert(0) += 1;
        }
    }
    for (name, count) in seen {
        if count > 1 {
            errors.push(ValidationError {
                kind: ValidationErrorKind::DuplicateOperationName,
                message: format!(
                    "操作名 `{}` が文書内で{}回定義されています(一意である必要があります)",
                    name, count
                ),
            });
        }
    }
}

// --- 5.8.3 Variables Are Used / 5.8.4 All Variable Uses Defined ---

fn collect_variables_from_value(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Variable(name) => {
            out.insert(name.clone());
        }
        Value::List(items) => {
            for item in items {
                collect_variables_from_value(item, out);
            }
        }
        Value::Object(fields) => {
            for (_, v) in fields {
                collect_variables_from_value(v, out);
            }
        }
        _ => {}
    }
}

fn collect_variables_from_directives(directives: &[Directive], out: &mut HashSet<String>) {
    for directive in directives {
        for arg in &directive.arguments {
            collect_variables_from_value(&arg.value, out);
        }
    }
}

fn collect_variables_from_selection_set(
    set: &SelectionSet,
    fragments: &HashMap<&str, &FragmentDefinition>,
    visited_fragments: &mut HashSet<String>,
    out: &mut HashSet<String>,
) {
    for sel in &set.selections {
        match sel {
            Selection::Field(f) => {
                for arg in &f.arguments {
                    collect_variables_from_value(&arg.value, out);
                }
                collect_variables_from_directives(&f.directives, out);
                if let Some(sub) = &f.selection_set {
                    collect_variables_from_selection_set(sub, fragments, visited_fragments, out);
                }
            }
            Selection::InlineFragment(inline) => {
                collect_variables_from_directives(&inline.directives, out);
                collect_variables_from_selection_set(
                    &inline.selection_set,
                    fragments,
                    visited_fragments,
                    out,
                );
            }
            Selection::FragmentSpread(spread) => {
                collect_variables_from_directives(&spread.directives, out);
                if visited_fragments.insert(spread.name.clone()) {
                    if let Some(frag) = fragments.get(spread.name.as_str()) {
                        collect_variables_from_directives(&frag.directives, out);
                        collect_variables_from_selection_set(
                            &frag.selection_set,
                            fragments,
                            visited_fragments,
                            out,
                        );
                    }
                }
            }
        }
    }
}

fn validate_variables(
    document: &Document,
    fragments: &HashMap<&str, &FragmentDefinition>,
    errors: &mut Vec<ValidationError>,
) {
    for op in &document.operations {
        let declared: HashSet<&str> = op
            .variable_definitions
            .iter()
            .map(|v| v.name.as_str())
            .collect();

        let mut used: HashSet<String> = HashSet::new();
        collect_variables_from_directives(&op.directives, &mut used);
        let mut visited_fragments = HashSet::new();
        collect_variables_from_selection_set(
            &op.selection_set,
            fragments,
            &mut visited_fragments,
            &mut used,
        );

        let label = operation_label(op);

        let mut used_sorted: Vec<&String> = used.iter().collect();
        used_sorted.sort();
        for used_name in used_sorted {
            if !declared.contains(used_name.as_str()) {
                errors.push(ValidationError {
                    kind: ValidationErrorKind::UndefinedVariable,
                    message: format!(
                        "{}内で使用されている変数 `${}` はこの操作の変数定義リストに存在しません",
                        label, used_name
                    ),
                });
            }
        }

        let mut declared_sorted: Vec<&str> = declared.iter().copied().collect();
        declared_sorted.sort();
        for declared_name in declared_sorted {
            if !used.contains(declared_name) {
                errors.push(ValidationError {
                    kind: ValidationErrorKind::UnusedVariable,
                    message: format!(
                        "{}で宣言されている変数 `${}` は使用されていません",
                        label, declared_name
                    ),
                });
            }
        }
    }
}

// --- 5.3.2 Field Selection Merging(部分集合) ---

fn validate_field_selection_merging(document: &Document, errors: &mut Vec<ValidationError>) {
    for op in &document.operations {
        check_selection_set_merging(&op.selection_set, errors);
    }
    for frag in &document.fragments {
        check_selection_set_merging(&frag.selection_set, errors);
    }
}

fn check_selection_set_merging(set: &SelectionSet, errors: &mut Vec<ValidationError>) {
    let mut seen: HashMap<&str, &Field> = HashMap::new();
    for sel in &set.selections {
        if let Selection::Field(f) = sel {
            let key = f.alias.as_deref().unwrap_or(f.name.as_str());
            if let Some(prev) = seen.get(key) {
                if prev.name != f.name {
                    errors.push(ValidationError {
                        kind: ValidationErrorKind::ConflictingFieldSelection,
                        message: format!(
                            "レスポンスキー `{}` が異なるフィールド(`{}` と `{}`)を指しています",
                            key, prev.name, f.name
                        ),
                    });
                } else if prev.arguments != f.arguments {
                    errors.push(ValidationError {
                        kind: ValidationErrorKind::ConflictingFieldSelection,
                        message: format!(
                            "レスポンスキー `{}` (フィールド `{}`) が異なる引数で複数回選択されています",
                            key, f.name
                        ),
                    });
                }
            } else {
                seen.insert(key, f);
            }
        }
    }

    for sel in &set.selections {
        match sel {
            Selection::Field(f) => {
                if let Some(sub) = &f.selection_set {
                    check_selection_set_merging(sub, errors);
                }
            }
            Selection::InlineFragment(inline) => {
                check_selection_set_merging(&inline.selection_set, errors);
            }
            Selection::FragmentSpread(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn errors_of(src: &str) -> Vec<ValidationError> {
        let doc = parse(src).expect("パースに失敗しました(テストの前提が壊れています)");
        validate(&doc)
    }

    fn has_kind(errors: &[ValidationError], kind: ValidationErrorKind) -> bool {
        errors.iter().any(|e| e.kind == kind)
    }

    // --- Fragment Spread Target Defined ---

    #[test]
    fn rejects_spread_of_undefined_fragment() {
        let errors = errors_of("{ hero { ...MissingFields } }");
        assert!(has_kind(&errors, ValidationErrorKind::UndefinedFragment));
    }

    #[test]
    fn accepts_spread_of_defined_fragment() {
        let errors = errors_of(
            r#"
            { hero { ...HeroFields } }
            fragment HeroFields on Character { name appearsIn }
            "#,
        );
        assert!(!has_kind(&errors, ValidationErrorKind::UndefinedFragment));
    }

    // --- Fragments Must Not Form Cycles ---

    #[test]
    fn rejects_direct_fragment_self_cycle() {
        let errors = errors_of(
            r#"
            { hero { ...A } }
            fragment A on Character { ...A }
            "#,
        );
        assert!(has_kind(&errors, ValidationErrorKind::FragmentCycle));
    }

    #[test]
    fn rejects_indirect_fragment_cycle() {
        let errors = errors_of(
            r#"
            { hero { ...A } }
            fragment A on Character { name ...B }
            fragment B on Character { name ...A }
            "#,
        );
        assert!(has_kind(&errors, ValidationErrorKind::FragmentCycle));
    }

    #[test]
    fn accepts_non_cyclic_fragment_chain() {
        let errors = errors_of(
            r#"
            { hero { ...A } }
            fragment A on Character { name ...B }
            fragment B on Character { appearsIn }
            "#,
        );
        assert!(!has_kind(&errors, ValidationErrorKind::FragmentCycle));
    }

    // --- Fragments Must Be Used ---

    #[test]
    fn rejects_unused_fragment_definition() {
        let errors = errors_of(
            r#"
            { hero { name } }
            fragment Unused on Character { appearsIn }
            "#,
        );
        assert!(has_kind(&errors, ValidationErrorKind::UnusedFragment));
    }

    #[test]
    fn accepts_used_fragment_definition() {
        let errors = errors_of(
            r#"
            { hero { ...HeroFields } }
            fragment HeroFields on Character { name }
            "#,
        );
        assert!(!has_kind(&errors, ValidationErrorKind::UnusedFragment));
    }

    #[test]
    fn accepts_fragment_used_only_transitively_through_another_fragment() {
        let errors = errors_of(
            r#"
            { hero { ...A } }
            fragment A on Character { name ...B }
            fragment B on Character { appearsIn }
            "#,
        );
        assert!(!has_kind(&errors, ValidationErrorKind::UnusedFragment));
    }

    // --- Lone Anonymous Operation ---

    #[test]
    fn rejects_anonymous_operation_alongside_named_operation() {
        let errors = errors_of("{ hero } query Other { droid }");
        assert!(has_kind(
            &errors,
            ValidationErrorKind::LoneAnonymousOperation
        ));
    }

    #[test]
    fn accepts_lone_anonymous_operation() {
        let errors = errors_of("{ hero }");
        assert!(!has_kind(
            &errors,
            ValidationErrorKind::LoneAnonymousOperation
        ));
    }

    #[test]
    fn accepts_multiple_named_operations_without_anonymous() {
        let errors = errors_of("query A { hero } query B { droid }");
        assert!(!has_kind(
            &errors,
            ValidationErrorKind::LoneAnonymousOperation
        ));
    }

    // --- Operation Name Uniqueness ---

    #[test]
    fn rejects_duplicate_operation_names() {
        let errors = errors_of("query Hero { hero } query Hero { droid }");
        assert!(has_kind(
            &errors,
            ValidationErrorKind::DuplicateOperationName
        ));
    }

    #[test]
    fn accepts_distinct_operation_names() {
        let errors = errors_of("query Hero { hero } query Droid { droid }");
        assert!(!has_kind(
            &errors,
            ValidationErrorKind::DuplicateOperationName
        ));
    }

    // --- All Variable Uses Defined ---

    #[test]
    fn rejects_undefined_variable_usage() {
        let errors = errors_of("query Q { hero(id: $missing) }");
        assert!(has_kind(&errors, ValidationErrorKind::UndefinedVariable));
    }

    #[test]
    fn accepts_variable_usage_declared_on_operation() {
        let errors = errors_of("query Q($id: ID!) { hero(id: $id) }");
        assert!(!has_kind(&errors, ValidationErrorKind::UndefinedVariable));
    }

    #[test]
    fn accepts_variable_used_transitively_through_fragment() {
        let errors = errors_of(
            r#"
            query Q($skipName: Boolean!) { hero { ...HeroFields } }
            fragment HeroFields on Character { name @skip(if: $skipName) }
            "#,
        );
        assert!(!has_kind(&errors, ValidationErrorKind::UndefinedVariable));
        assert!(!has_kind(&errors, ValidationErrorKind::UnusedVariable));
    }

    // --- Variables Are Used ---

    #[test]
    fn rejects_unused_declared_variable() {
        let errors = errors_of("query Q($id: ID!) { hero }");
        assert!(has_kind(&errors, ValidationErrorKind::UnusedVariable));
    }

    #[test]
    fn accepts_declared_variable_used_in_nested_argument_object() {
        let errors = errors_of("query Q($id: ID!) { hero(filter: {id: $id}) }");
        assert!(!has_kind(&errors, ValidationErrorKind::UnusedVariable));
    }

    // --- Field Selection Merging(部分集合) ---

    #[test]
    fn rejects_conflicting_aliases_pointing_to_different_fields() {
        let errors = errors_of("{ hero { name: id name: title } }");
        assert!(has_kind(
            &errors,
            ValidationErrorKind::ConflictingFieldSelection
        ));
    }

    #[test]
    fn rejects_same_field_with_conflicting_arguments_at_same_response_key() {
        let errors = errors_of("{ hero(id: 1) hero(id: 2) }");
        assert!(has_kind(
            &errors,
            ValidationErrorKind::ConflictingFieldSelection
        ));
    }

    #[test]
    fn accepts_identical_repeated_field_selection() {
        let errors = errors_of("{ hero(id: 1) hero(id: 1) }");
        assert!(!has_kind(
            &errors,
            ValidationErrorKind::ConflictingFieldSelection
        ));
    }

    #[test]
    fn accepts_distinct_aliases_for_same_field() {
        let errors = errors_of("{ luke: hero(id: 1) leia: hero(id: 2) }");
        assert!(!has_kind(
            &errors,
            ValidationErrorKind::ConflictingFieldSelection
        ));
    }

    // --- 総合サニティチェック: 現実的な複数操作/複数フラグメントの
    // ドキュメントで、意図せぬfalse positiveが出ないことを確認する ---

    #[test]
    fn realistic_multi_operation_multi_fragment_document_is_fully_valid() {
        let errors = errors_of(
            r#"
            query HeroComparison($episode: Episode, $withFriends: Boolean! = true) {
                hero(episode: $episode) {
                    ...HeroFields
                    friends @include(if: $withFriends) {
                        ...HeroFields
                    }
                }
            }

            mutation LikeHero($storyID: ID!) {
                like(storyID: $storyID) {
                    likeCount
                }
            }

            fragment HeroFields on Character {
                id
                name
                appearsIn
                ... on Droid {
                    primaryFunction
                }
            }
            "#,
        );
        assert_eq!(errors, Vec::new(), "妥当なドキュメントでエラーが検出されました: {:?}", errors);
    }

    #[test]
    fn realistic_document_with_nested_fragments_and_inline_fragments_is_valid() {
        let errors = errors_of(
            r#"
            query FeedQuery {
                feed {
                    ...FeedEntryFields
                    ... on StoryEntry {
                        commentCount
                    }
                }
            }

            fragment FeedEntryFields on Entry {
                id
                ...EntryMeta
            }

            fragment EntryMeta on Entry {
                repository {
                    name
                }
            }
            "#,
        );
        assert_eq!(errors, Vec::new(), "妥当なドキュメントでエラーが検出されました: {:?}", errors);
    }

    #[test]
    fn realistic_named_multi_operation_document_with_variables_is_valid() {
        let errors = errors_of(
            r#"
            query GetHero($id: ID!) {
                human(id: $id) {
                    name
                    height
                }
            }

            query GetDroid($id: ID!) {
                droid(id: $id) {
                    name
                    primaryFunction
                }
            }
            "#,
        );
        assert_eq!(errors, Vec::new(), "妥当なドキュメントでエラーが検出されました: {:?}", errors);
    }
}
