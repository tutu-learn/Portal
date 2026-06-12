use crate::TargetDialect;
use crate::functions::rewrite_expr;
use crate::tables::rewrite_name;
use error::Result;
use sqlparser::ast::Statement;

pub fn rewrite(stmts: &mut Vec<Statement>, _target: TargetDialect) -> Result<()> {
    for stmt in stmts.iter_mut() {
        rewrite_statement(stmt)?;
    }
    Ok(())
}

fn rewrite_statement(stmt: &mut Statement) -> Result<()> {
    use sqlparser::ast::Statement;
    match stmt {
        Statement::Query(query) => {
            rewrite_query(query);
        }
        Statement::Insert(insert) => {
            if let sqlparser::ast::TableObject::TableName(name) = &mut insert.table {
                rewrite_name(name);
            }
            for col in &mut insert.columns {
                col.value = col.value.to_lowercase().replace(" ", "_");
                col.quote_style = None;
            }
            if let Some(src) = &mut insert.source {
                rewrite_query(src);
            }
            for assign in &mut insert.assignments {
                rewrite_assignment(assign);
            }
            if let Some(on) = &mut insert.on {
                rewrite_on_insert(on);
            }
            if let Some(ret) = &mut insert.returning {
                for item in ret {
                    rewrite_select_item(item);
                }
            }
        }
        Statement::Update { table, assignments, from, selection, returning, .. } => {
            rewrite_table_with_joins(table);
            for assign in assignments {
                rewrite_assignment(assign);
            }
            if let Some(from_kind) = from {
                match from_kind {
                    sqlparser::ast::UpdateTableFromKind::BeforeSet(t) |
                    sqlparser::ast::UpdateTableFromKind::AfterSet(t) => {
                        rewrite_table_with_joins(t);
                    }
                }
            }
            if let Some(sel) = selection {
                rewrite_expr(sel);
            }
            if let Some(ret) = returning {
                for item in ret {
                    rewrite_select_item(item);
                }
            }
        }
        Statement::Delete(delete) => {
            for t in &mut delete.tables {
                rewrite_name(t);
            }
            match &mut delete.from {
                sqlparser::ast::FromTable::WithFromKeyword(tables) |
                sqlparser::ast::FromTable::WithoutKeyword(tables) => {
                    for t in tables {
                        rewrite_table_factor(&mut t.relation);
                    }
                }
            }
            if let Some(using) = &mut delete.using {
                for t in using {
                    rewrite_table_factor(&mut t.relation);
                }
            }
            if let Some(sel) = &mut delete.selection {
                rewrite_expr(sel);
            }
            if let Some(ret) = &mut delete.returning {
                for item in ret {
                    rewrite_select_item(item);
                }
            }
        }
        Statement::CreateTable(create_table) => {
            rewrite_name(&mut create_table.name);
            for col in &mut create_table.columns {
                col.name.value = col.name.value.to_lowercase().replace(" ", "_");
                col.name.quote_style = None;
            }
            for c in &mut create_table.constraints {
                rewrite_table_constraint(c);
            }
        }
        Statement::CreateIndex(create_index) => {
            rewrite_name(&mut create_index.table_name);
        }
        Statement::Drop { names, .. } => {
            for name in names {
                rewrite_name(name);
            }
        }
        Statement::AlterTable { name, .. } => {
            rewrite_name(name);
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn rewrite_query(query: &mut sqlparser::ast::Query) {
    use sqlparser::ast::SetExpr;
    match &mut *query.body {
        SetExpr::Select(select) => {
            for item in &mut select.projection {
                rewrite_select_item(item);
            }
            if let Some(selection) = &mut select.selection {
                rewrite_expr(selection);
            }
            for table in &mut select.from {
                rewrite_table_factor(&mut table.relation);
                for join in &mut table.joins {
                    if let sqlparser::ast::TableFactor::Table { name, .. } = &mut join.relation {
                        rewrite_name(name);
                    }
                    match &mut join.join_operator {
                        sqlparser::ast::JoinOperator::Inner(constraint)
                        | sqlparser::ast::JoinOperator::LeftOuter(constraint)
                        | sqlparser::ast::JoinOperator::RightOuter(constraint)
                        | sqlparser::ast::JoinOperator::FullOuter(constraint) => {
                            if let sqlparser::ast::JoinConstraint::On(e) = constraint {
                                rewrite_expr(e);
                            }
                        }
                        _ => {}
                    }
                }
            }
            match &mut select.group_by {
                sqlparser::ast::GroupByExpr::All(_) => {}
                sqlparser::ast::GroupByExpr::Expressions(exprs, _) => {
                    for e in exprs {
                        rewrite_expr(e);
                    }
                }
            }
            if let Some(having) = &mut select.having {
                rewrite_expr(having);
            }
        }
        SetExpr::SetOperation { left, right, .. } => {
            rewrite_set_expr(left);
            rewrite_set_expr(right);
        }
        _ => {}
    }
    if let Some(order_by) = &mut query.order_by {
        for e in order_by.exprs.iter_mut() {
            rewrite_expr(&mut e.expr);
        }
    }
    if let Some(limit) = &mut query.limit {
        rewrite_expr(limit);
    }
    if let Some(offset) = &mut query.offset {
        rewrite_expr(&mut offset.value);
    }
}

fn rewrite_set_expr(expr: &mut sqlparser::ast::SetExpr) {
    use sqlparser::ast::SetExpr;
    match expr {
        SetExpr::Select(select) => {
            let mut q = sqlparser::ast::Query {
                with: None,
                body: Box::new(SetExpr::Select(select.clone())),
                order_by: None,
                limit: None,
                offset: None,
                fetch: None,
                locks: vec![],
                for_clause: None,
                settings: None,
                format_clause: None,
                limit_by: vec![],
            };
            rewrite_query(&mut q);
            if let SetExpr::Select(s) = *q.body {
                **select = *s;
            }
        }
        SetExpr::Query(q) => rewrite_query(q),
        SetExpr::SetOperation { left, right, .. } => {
            rewrite_set_expr(left);
            rewrite_set_expr(right);
        }
        _ => {}
    }
}

fn rewrite_table_factor(tf: &mut sqlparser::ast::TableFactor) {
    use sqlparser::ast::TableFactor;
    match tf {
        TableFactor::Table { name, .. } => {
            rewrite_name(name);
        }
        TableFactor::Derived { subquery, .. } => {
            rewrite_query(subquery);
        }
        _ => {}
    }
}

fn rewrite_table_with_joins(twj: &mut sqlparser::ast::TableWithJoins) {
    rewrite_table_factor(&mut twj.relation);
    for join in &mut twj.joins {
        rewrite_table_factor(&mut join.relation);
    }
}

fn rewrite_select_item(item: &mut sqlparser::ast::SelectItem) {
    match item {
        sqlparser::ast::SelectItem::UnnamedExpr(e)
        | sqlparser::ast::SelectItem::ExprWithAlias { expr: e, .. } => {
            rewrite_expr(e);
        }
        _ => {}
    }
}

fn rewrite_assignment(assign: &mut sqlparser::ast::Assignment) {
    match &mut assign.target {
        sqlparser::ast::AssignmentTarget::ColumnName(name) => {
            for ident in &mut name.0 {
                ident.value = ident.value.to_lowercase().replace(" ", "_");
                ident.quote_style = None;
            }
        }
        sqlparser::ast::AssignmentTarget::Tuple(names) => {
            for name in names {
                for ident in &mut name.0 {
                    ident.value = ident.value.to_lowercase().replace(" ", "_");
                    ident.quote_style = None;
                }
            }
        }
    }
    rewrite_expr(&mut assign.value);
}

fn rewrite_on_insert(on: &mut sqlparser::ast::OnInsert) {
    use sqlparser::ast::OnInsert;
    match on {
        OnInsert::DuplicateKeyUpdate(assignments) => {
            for assign in assignments {
                rewrite_assignment(assign);
            }
        }
        _ => {}
    }
}

fn rewrite_table_constraint(c: &mut sqlparser::ast::TableConstraint) {
    use sqlparser::ast::TableConstraint;
    match c {
        TableConstraint::Unique { columns, .. }
        | TableConstraint::PrimaryKey { columns, .. } => {
            for col in columns {
                col.value = col.value.to_lowercase().replace(" ", "_");
                col.quote_style = None;
            }
        }
        TableConstraint::ForeignKey {
            columns,
            referred_columns,
            ..
        } => {
            for col in columns {
                col.value = col.value.to_lowercase().replace(" ", "_");
                col.quote_style = None;
            }
            for col in referred_columns {
                col.value = col.value.to_lowercase().replace(" ", "_");
                col.quote_style = None;
            }
        }
        TableConstraint::Check { expr, .. } => {
            rewrite_expr(expr);
        }
        _ => {}
    }
}
