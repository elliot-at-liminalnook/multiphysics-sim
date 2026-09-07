//! Exact bipartite partition of a freshly assembled dependent matrix.
//! A zero at one pose never authorizes reuse of that sparsity at another pose.
use nalgebra::DMatrix;

pub(super) fn independent_blocks(a: &DMatrix<f64>) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut parent: Vec<_> = (0..a.ncols()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first = vec![None; a.nrows()];
    for row in 0..a.nrows() {
        for col in 0..a.ncols() {
            if a[(row, col)] == 0.0 {
                continue;
            }
            if let Some(previous) = first[row] {
                let x = root(&mut parent, previous);
                let y = root(&mut parent, col);
                parent[y] = x;
            } else {
                first[row] = Some(col);
            }
        }
    }
    let mut groups = std::collections::BTreeMap::<usize, (Vec<usize>, Vec<usize>)>::new();
    for col in 0..a.ncols() {
        let key = root(&mut parent, col);
        groups.entry(key).or_default().1.push(col);
    }
    for (row, col) in first.into_iter().enumerate() {
        if let Some(col) = col {
            groups.get_mut(&root(&mut parent, col)).unwrap().0.push(row);
        }
    }
    // Entirely zero rows contribute no coefficient to a least-squares solution.
    // Callers still check every original position/velocity/acceleration equation.
    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repartitions_every_matrix_and_never_discards_tiny_couplings() {
        let mut a =
            DMatrix::from_row_slice(4, 3, &[1., 0., 0., 0., 0., 2., 0., 3., 0., 0., 0., 0.]);
        assert_eq!(independent_blocks(&a).len(), 3);
        a[(0, 1)] = 1e-300;
        assert_eq!(independent_blocks(&a).len(), 2);
        a[(1, 1)] = -1e-300;
        assert_eq!(independent_blocks(&a).len(), 1);
        a.fill(0.0);
        assert_eq!(
            independent_blocks(&a),
            vec![(vec![], vec![0]), (vec![], vec![1]), (vec![], vec![2])]
        );
    }
}

#[cfg(test)]
mod factor_tests {
    use super::super::{DependentSolve, EmbeddingConfig, RigidEmbedding};
    use nalgebra::DMatrix;
    #[test]
    fn block_least_squares_matches_whole_matrix_and_keeps_global_rank_threshold() {
        let scene: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/interactive/pendulum.scene.json"
        )))
        .unwrap();
        let art = crate::Articulated::new(
            std::sync::Arc::new(serde_json::from_value(scene["robot"].clone()).unwrap()),
            &Default::default(),
        )
        .unwrap();
        for method in [DependentSolve::Svd, DependentSolve::PivotedQr] {
            let mut map = RigidEmbedding::new(
                &art,
                &[],
                EmbeddingConfig {
                    dependent_solve: method,
                    ..Default::default()
                },
            )
            .unwrap();
            // Exercise the private matrix kernel independently of mechanism topology.
            map.dependent = vec![0, 1, 2];
            let a = DMatrix::from_row_slice(
                6,
                3,
                &[
                    2., 0., 0., 0., 3., 0., 4., 0., 0., 0., 0., 0.4, 0., 6., 0., 0., 0., 0.,
                ],
            );
            let rhs = DMatrix::from_row_slice(
                6,
                2,
                &[1., 2., 4., -3., 3., 8., -2., 1., 5., 7., 11., 12.],
            );
            let full = map.factor_dependent(&a).unwrap();
            map.config.block_dependent_factorization = true;
            let block = map.factor_dependent(&a).unwrap();
            assert!((full.0.solve(&rhs).unwrap() - block.0.solve(&rhs).unwrap()).amax() < 1e-12);
            assert!((full.1 - block.1).abs() < 1e-12);
            let ill_scaled =
                DMatrix::from_diagonal(&nalgebra::DVector::from_vec(vec![1e12, 1., 1.]));
            assert!(matches!(map.factor_dependent(&ill_scaled), Err(e) if e.contains("singular")));
        }
    }
}
