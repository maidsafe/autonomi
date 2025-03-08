// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::Deserialize;
use std::{collections::HashMap, str};

/// Example JSON representing the equation `a + b`
pub const PLUS_EQUATION: &str = r#"
    {
      "type": "op",
      "op": "+",
      "args": [
        { "type": "param", "name": "a" },
        { "type": "param", "name": "b" }
      ]
    }
    "#;

/// Example JSON representing the equation `(a + 5) * b`
pub const COMPLEX_EQUATION: &str = r#"{
      "type": "op",
      "op": "*",
      "args": [
        {
          "type": "op",
          "op": "+",
          "args": [
            { "type": "param", "name": "a" },
            { "type": "const", "value": 5 }
          ]
        },
        { "type": "param", "name": "b" }
      ]
    }
    "#;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Expr {
    Const { value: f64 },
    Param { name: String },
    Op { op: String, args: Vec<Expr> },
}

fn evaluate(expr: &Expr, params: &HashMap<String, f64>) -> Result<f64, String> {
    match expr {
        Expr::Const { value } => Ok(*value),
        Expr::Param { name } => params
            .get(name)
            .copied()
            .ok_or_else(|| format!("Parameter '{name}' not found")),
        Expr::Op { op, args } => {
            let evaluated_args: Result<Vec<f64>, String> =
                args.iter().map(|arg| evaluate(arg, params)).collect();
            let args = evaluated_args?;
            match op.as_str() {
                "+" => Ok(args.iter().sum()),
                "*" => Ok(args.iter().product()),
                _ => Err(format!("Unknown operator: {op}")),
            }
        }
    }
}

pub fn compute(params: HashMap<String, f64>, equation: &str) -> Result<f64, String> {
    let expr: Expr =
        serde_json::from_str(equation).map_err(|e| format!("Failed to parse JSON: {e}"))?;
    evaluate(&expr, &params)
}
