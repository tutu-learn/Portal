use sqlparser::ast::{
    Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, ObjectName,
};

pub fn rewrite_expr(expr: &mut Expr) {
    match expr {
        Expr::Function(func) => rewrite_function(func),
        Expr::Nested(e) => rewrite_expr(e),
        Expr::UnaryOp { expr, .. } => rewrite_expr(expr),
        Expr::BinaryOp { left, right, .. } => {
            rewrite_expr(left);
            rewrite_expr(right);
        }
        Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            if let Some(op) = operand {
                rewrite_expr(op);
            }
            for cond in conditions {
                rewrite_expr(cond);
            }
            for res in results {
                rewrite_expr(res);
            }
            if let Some(el) = else_result {
                rewrite_expr(el);
            }
        }
        Expr::Cast { expr, .. } => rewrite_expr(expr),
        Expr::InList { expr, list, .. } => {
            rewrite_expr(expr);
            for item in list {
                rewrite_expr(item);
            }
        }
        Expr::InSubquery { expr, subquery, .. } => {
            rewrite_expr(expr);
            crate::rewriter::rewrite_query(subquery);
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            rewrite_expr(expr);
            rewrite_expr(low);
            rewrite_expr(high);
        }
        Expr::Subquery(query) => crate::rewriter::rewrite_query(query),
        Expr::Exists { subquery, .. } => crate::rewriter::rewrite_query(subquery),
        Expr::IsNull(expr) => rewrite_expr(expr),
        Expr::IsNotNull(expr) => rewrite_expr(expr),
        Expr::IsDistinctFrom(left, right) => {
            rewrite_expr(left);
            rewrite_expr(right);
        }
        Expr::IsNotDistinctFrom(left, right) => {
            rewrite_expr(left);
            rewrite_expr(right);
        }
        Expr::Like { expr, pattern, .. } => {
            rewrite_expr(expr);
            rewrite_expr(pattern);
        }
        Expr::ILike { expr, pattern, .. } => {
            rewrite_expr(expr);
            rewrite_expr(pattern);
        }
        Expr::SimilarTo { expr, pattern, .. } => {
            rewrite_expr(expr);
            rewrite_expr(pattern);
        }
        Expr::AnyOp { left, right, .. } | Expr::AllOp { left, right, .. } => {
            rewrite_expr(left);
            rewrite_expr(right);
        }
        Expr::Array(array) => {
            for e in &mut array.elem {
                rewrite_expr(e);
            }
        }
        Expr::Position { expr, r#in } => {
            rewrite_expr(expr);
            rewrite_expr(r#in);
        }
        Expr::AtTimeZone {
            timestamp,
            time_zone,
        } => {
            rewrite_expr(timestamp);
            rewrite_expr(time_zone);
        }
        Expr::Interval(interval) => {
            rewrite_expr(&mut interval.value);
        }
        Expr::Overlay {
            expr,
            overlay_what,
            overlay_from,
            overlay_for,
        } => {
            rewrite_expr(expr);
            rewrite_expr(overlay_what);
            rewrite_expr(overlay_from);
            if let Some(overlay_for) = overlay_for {
                rewrite_expr(overlay_for);
            }
        }
        Expr::Extract { expr, .. } => {
            rewrite_expr(expr);
        }
        Expr::Tuple(exprs) => {
            for e in exprs {
                rewrite_expr(e);
            }
        }
        Expr::Struct { values, .. } => {
            for e in values {
                rewrite_expr(e);
            }
        }
        Expr::Named { expr, .. } => {
            rewrite_expr(expr);
        }
        Expr::Dictionary(fields) => {
            for field in fields {
                rewrite_expr(&mut field.value);
            }
        }
        Expr::Substring {
            expr,
            substring_from,
            substring_for,
            special: _,
        } => {
            rewrite_expr(expr);
            if let Some(from) = substring_from {
                rewrite_expr(from);
            }
            if let Some(for_expr) = substring_for {
                rewrite_expr(for_expr);
            }
        }
        Expr::Wildcard(_) => {}
        _ => {}
    }
}

fn rewrite_function(func: &mut Function) {
    let name_upper = func
        .name
        .0
        .iter()
        .map(|i| i.value.to_uppercase())
        .collect::<Vec<_>>()
        .join(".");

    match name_upper.as_str() {
        "IFNULL" => {
            func.name = ObjectName(vec![Ident::new("COALESCE")]);
        }
        "NOW" => {
            func.name = ObjectName(vec![Ident::new("CURRENT_TIMESTAMP")]);
            func.args = FunctionArguments::None;
        }
        _ => {}
    }

    if let FunctionArguments::List(list) = &mut func.args {
        for arg in &mut list.args {
            if let FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) = arg {
                rewrite_expr(e);
            } else if let FunctionArg::Named { arg, .. } = arg {
                if let FunctionArgExpr::Expr(e) = arg {
                    rewrite_expr(e);
                }
            }
        }
    } else if let FunctionArguments::Subquery(query) = &mut func.args {
        crate::rewriter::rewrite_query(query);
    }
}
