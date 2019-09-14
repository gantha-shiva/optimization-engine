#![allow(dead_code)]

use crate::{
    alm::*,
    constraints,
    core::{panoc::PANOCOptimizer, Optimizer, Problem, SolverStatus},
    matrix_operations, SolverError,
};

const DEFAULT_MAX_OUTER_ITERATIONS: usize = 50;
const DEFAULT_MAX_INNER_ITERATIONS: usize = 5000;
const DEFAULT_EPSILON_TOLERANCE: f64 = 1e-6;
const DEFAULT_DELTA_TOLERANCE: f64 = 1e-4;
const DEFAULT_PENALTY_UPDATE_FACTOR: f64 = 5.0;
const DEFAULT_EPSILON_UPDATE_FACTOR: f64 = 0.1;
const DEFAULT_INFEAS_SUFFICIENT_DECREASE_FACTOR: f64 = 0.1;
const DEFAULT_INITIAL_TOLERANCE: f64 = 0.1;

pub struct AlmOptimizer<
    'life,
    MappingAlm,
    MappingPm,
    ParametricGradientType,
    ConstraintsType,
    AlmSetC,
    LagrangeSetY,
    ParametricCostType,
> where
    MappingAlm: Fn(&[f64], &mut [f64]) -> Result<(), SolverError>,
    MappingPm: Fn(&[f64], &mut [f64]) -> Result<(), SolverError>,
    ParametricGradientType: Fn(&[f64], &[f64], &mut [f64]) -> Result<(), SolverError>,
    ParametricCostType: Fn(&[f64], &[f64], &mut f64) -> Result<(), SolverError>,
    ConstraintsType: constraints::Constraint,
    AlmSetC: constraints::Constraint,
    LagrangeSetY: constraints::Constraint,
{
    /// ALM cache (borrowed)
    alm_cache: &'life mut AlmCache,
    /// ALM problem definition (oracle)
    alm_problem: AlmProblem<
        MappingAlm,
        MappingPm,
        ParametricGradientType,
        ConstraintsType,
        AlmSetC,
        LagrangeSetY,
        ParametricCostType,
    >,
    /// Maximum number of outer iterations
    max_outer_iterations: usize,
    /// Maximum number of inner iterations
    max_inner_iterations: usize,
    /// Maximum duration
    max_duration: Option<std::time::Duration>,
    /// epsilon for inner AKKT condition
    epsilon_tolerance: f64,
    /// delta for outer AKKT condition
    delta_tolerance: f64,
    /// At every outer iteration, c is multiplied by this scalar
    penalty_update_factor: f64,
    /// The epsilon-tolerance is multiplied by this factor until
    /// it reaches its target value
    epsilon_update_factor: f64,
    /// If current_infeasibility <= sufficient_decrease_coeff * previous_infeasibility,
    /// then the penalty parameter is kept constant
    sufficient_decrease_coeff: f64,
    // Initial tolerance (for the inner problem)
    epsilon_inner_initial: f64,
}

impl<
        'life,
        MappingAlm,
        MappingPm,
        ParametricGradientType,
        ConstraintsType,
        AlmSetC,
        LagrangeSetY,
        ParametricCostType,
    >
    AlmOptimizer<
        'life,
        MappingAlm,
        MappingPm,
        ParametricGradientType,
        ConstraintsType,
        AlmSetC,
        LagrangeSetY,
        ParametricCostType,
    >
where
    MappingAlm: Fn(&[f64], &mut [f64]) -> Result<(), SolverError>,
    MappingPm: Fn(&[f64], &mut [f64]) -> Result<(), SolverError>,
    ParametricGradientType: Fn(&[f64], &[f64], &mut [f64]) -> Result<(), SolverError>,
    ParametricCostType: Fn(&[f64], &[f64], &mut f64) -> Result<(), SolverError>,
    ConstraintsType: constraints::Constraint,
    AlmSetC: constraints::Constraint,
    LagrangeSetY: constraints::Constraint,
{
    /// Create new instance of `AlmOptimizer`
    ///
    /// ## Arguments
    ///
    /// ## Example
    ///
    /// ```rust
    /// use optimization_engine::{alm::*, SolverError, core::{panoc::*, constraints}};
    ///
    /// let tolerance = 1e-8;
    /// let nx = 10;
    /// let n1 = 5;
    /// let n2 = 0;
    /// let lbfgs_mem = 3;
    /// let panoc_cache = PANOCCache::new(nx, tolerance, lbfgs_mem);
    /// let mut alm_cache = AlmCache::new(panoc_cache, n1, n2);
    ///
    /// let f = |_u: &[f64], _p: &[f64], _cost: &mut f64| -> Result<(), SolverError> { Ok(()) };
    /// let df = |_u: &[f64], _p: &[f64], _grad: &mut [f64]| -> Result<(), SolverError> { Ok(()) };
    /// let f1 = |_u: &[f64], _grad: &mut [f64]| -> Result<(), SolverError> { Ok(()) };
    /// let set_c = constraints::Ball2::new(None, 1.50);
    ///
    /// // Construct an instance of AlmProblem without any PM-type data
    /// let bounds = constraints::Ball2::new(None, 10.0);
    /// let set_y = constraints::Ball2::new(None, 1.0);
    /// let alm_problem = AlmProblem::new(
    ///     bounds,
    ///     Some(set_c),
    ///     Some(set_y),
    ///     f,
    ///     df,
    ///     Some(f1),
    ///     NO_MAPPING,
    ///     n1,
    ///     n2,
    /// );
    ///
    /// let mut alm_optimizer = AlmOptimizer::new(&mut alm_cache, alm_problem)
    ///     .with_delta_tolerance(1e-4)
    ///     .with_max_outer_iterations(10);
    ///```     
    ///
    pub fn new(
        alm_cache: &'life mut AlmCache,
        alm_problem: AlmProblem<
            MappingAlm,
            MappingPm,
            ParametricGradientType,
            ConstraintsType,
            AlmSetC,
            LagrangeSetY,
            ParametricCostType,
        >,
    ) -> Self {
        // set the initial value of the inner tolerance
        alm_cache
            .panoc_cache
            .set_akkt_tolerance(DEFAULT_INITIAL_TOLERANCE);
        AlmOptimizer {
            alm_cache,
            alm_problem,
            max_outer_iterations: DEFAULT_MAX_OUTER_ITERATIONS,
            max_inner_iterations: DEFAULT_MAX_INNER_ITERATIONS,
            max_duration: None,
            epsilon_tolerance: DEFAULT_EPSILON_TOLERANCE,
            delta_tolerance: DEFAULT_DELTA_TOLERANCE,
            penalty_update_factor: DEFAULT_PENALTY_UPDATE_FACTOR,
            epsilon_update_factor: DEFAULT_EPSILON_UPDATE_FACTOR,
            sufficient_decrease_coeff: DEFAULT_INFEAS_SUFFICIENT_DECREASE_FACTOR,
            epsilon_inner_initial: DEFAULT_INITIAL_TOLERANCE,
        }
    }

    pub fn with_max_outer_iterations(mut self, max_outer_iterations: usize) -> Self {
        self.max_outer_iterations = max_outer_iterations;
        self
    }

    pub fn with_max_inner_iterations(mut self, max_inner_iterations: usize) -> Self {
        self.max_inner_iterations = max_inner_iterations;
        self
    }

    pub fn with_max_duration(mut self, max_duration: std::time::Duration) -> Self {
        self.max_duration = Some(max_duration);
        self
    }

    pub fn with_delta_tolerance(mut self, delta_tolerance: f64) -> Self {
        self.delta_tolerance = delta_tolerance;
        self
    }

    pub fn with_epsilon_tolerance(mut self, epsilon_tolerance: f64) -> Self {
        self.epsilon_tolerance = epsilon_tolerance;
        self
    }

    pub fn with_penalty_update_factor(mut self, penalty_update_factor: f64) -> Self {
        self.penalty_update_factor = penalty_update_factor;
        self
    }

    pub fn with_inner_tolerance_update_factor(
        mut self,
        inner_tolerance_update_factor: f64,
    ) -> Self {
        self.epsilon_update_factor = inner_tolerance_update_factor;
        self
    }

    pub fn with_sufficient_decrease_coefficient(
        mut self,
        sufficient_decrease_coefficient: f64,
    ) -> Self {
        self.sufficient_decrease_coeff = sufficient_decrease_coefficient;
        self
    }

    pub fn with_initial_inner_tolerance(mut self, initial_inner_tolerance: f64) -> Self {
        self.epsilon_inner_initial = initial_inner_tolerance;
        self
    }

    pub fn with_initial_lagrange_multipliers(mut self, y_init: &[f64]) -> Self {
        let cache = &mut self.alm_cache;
        assert!(
            y_init.len() == self.alm_problem.n1,
            "y_init has wrong length"
        );
        if let Some(xi_in_cache) = &mut cache.xi {
            xi_in_cache[1..].copy_from_slice(y_init);
        }
        self
    }

    pub fn with_initial_penalty(self, c0: f64) -> Self {
        if let Some(xi_in_cache) = &mut self.alm_cache.xi {
            xi_in_cache[0] = c0;
        }
        self
    }

    /* ---------------------------------------------------------------------------- */
    /*          PRIVATE METHODS                                                     */
    /* ---------------------------------------------------------------------------- */

    fn compute_alm_infeasibility(&mut self) -> Result<(), SolverError> {
        let alm_cache = &mut self.alm_cache; // ALM cache
        if let (Some(y_plus), Some(xi)) = (&alm_cache.y_plus, &alm_cache.xi) {
            let norm_diff_squared: f64 = matrix_operations::norm2_squared_diff(&y_plus, &xi[1..]);
            alm_cache.delta_y_norm_plus = norm_diff_squared.sqrt();
        }
        Ok(())
    }

    /// Computes PM infeasibility, that is, ||F2(u)||
    fn compute_pm_infeasibility(&mut self, u: &[f64]) -> Result<(), SolverError> {
        let problem = &self.alm_problem; // ALM problem
        let cache = &mut self.alm_cache; // ALM cache

        // If there is an F2 mapping: cache.w_pm <-- F2
        // Then compute the norm of w_pm and store it in cache.f2_norm_plus
        if let (Some(f2), Some(w_pm_vec)) = (&problem.mapping_f2, &mut cache.w_pm.as_mut()) {
            f2(u, w_pm_vec)?;
            cache.f2_norm_plus = matrix_operations::norm2(w_pm_vec);
        }
        Ok(())
    }

    /// Updates the Lagrange multipliers using
    ///
    /// `y_plus <-- y + c*[F1(u_plus) - Proj_C(F1(u_plus) + y/c)]`
    ///
    fn update_lagrange_multipliers(&mut self, u: &[f64]) -> Result<(), SolverError> {
        let problem = &self.alm_problem; // ALM problem
        let cache = &mut self.alm_cache; // ALM cache

        // y_plus <-- y + c*[F1(u_plus) - Proj_C(F1(u_plus) + y/c)]
        // This is implemented as follows:
        //
        // #1. w_alm_aux := F1(u), where u = solution of inner problem
        // #2. y_plus := w_alm_aux + y/c
        // #3. y_plus := Proj_C(y_plus)
        // #4. y_plus := y + c(w_alm_aux - y_plus)

        // Before we start: this should not be executed if n1 = 0
        if problem.n1 == 0 {
            return Ok(()); // nothing to do (no ALM), return
        }

        if let (Some(f1), Some(w_alm_aux), Some(y_plus), Some(xi), Some(alm_set_c)) = (
            &problem.mapping_f1,
            &mut cache.w_alm_aux,
            &mut cache.y_plus,
            &mut cache.xi,
            &problem.alm_set_c,
        ) {
            // Step #1: w_alm_aux := F1(u)
            (f1)(u, w_alm_aux)?;

            // Step #2: y_plus := w_alm_aux + y/c
            let y = &xi[1..];
            let c = xi[0];
            y_plus
                .iter_mut()
                .zip(y.iter())
                .zip(w_alm_aux.iter())
                .for_each(|((y_plus_i, y_i), w_alm_aux_i)| *y_plus_i = w_alm_aux_i + y_i / c);

            // Step #3: y_plus := Proj_C(y_plus)
            alm_set_c.project(y_plus);

            // Step #4
            y_plus
                .iter_mut()
                .zip(y.iter())
                .zip(w_alm_aux.iter())
                .for_each(|((y_plus_i, y_i), w_alm_aux_i)| {
                    // y_plus := y  + c * (w_alm_aux   - y_plus)
                    *y_plus_i = y_i + c * (w_alm_aux_i - *y_plus_i)
                });
        }

        Ok(())
    }

    /// Project y on set Y
    fn project_on_set_y(&mut self) {
        let problem = &self.alm_problem;
        if let Some(y_set) = &problem.alm_set_y {
            // NOTE: as_mut() converts from &mut Option<T> to Option<&mut T>
            // * cache.y is                Option<Vec<f64>>
            // * cache.y.as_mut is         Option<&mut Vec<f64>>
            // *  which can be treated as  Option<&mut [f64]>
            // * y_vec is                  &mut [f64]
            if let Some(xi_vec) = self.alm_cache.xi.as_mut() {
                y_set.project(&mut xi_vec[1..]);
            }
        }
    }

    /// Solve inner problem
    ///
    /// ## Arguments
    ///
    /// - `u`: (on entry) current iterate, `u^nu`, (on exit) next iterate,
    ///   `u^{nu+1}` which is an epsilon-approximate solution of the inner problem
    /// - `xi`: vector `xi = (c, y)`
    ///
    /// ## Returns
    ///
    /// Returns an instance of `Result<SolverStatus, SolverError>`, where `SolverStatus`
    /// is the solver status of the inner problem and `SolverError` is a potential
    /// error in solving the inner problem.
    ///
    ///
    fn solve_inner_problem(&mut self, u: &mut [f64]) -> Result<SolverStatus, SolverError> {
        let alm_problem = &self.alm_problem; // Problem
        let alm_cache = &mut self.alm_cache; // ALM cache

        // `xi` is either the cached `xi` if one exists, or an reference to an
        // empty vector, otherwise. We do that becaues the user has the option
        // to not use any ALM/PM constraints; in that case, `alm_cache.xi` is
        // `None`
        let xi_empty = Vec::new();
        let xi = if let Some(xi_cached) = &alm_cache.xi {
            &xi_cached
        } else {
            &xi_empty
        };
        // Construct psi and psi_grad (as functions of `u` alone); it is
        // psi(u) = psi(u; xi) and psi_grad(u) = phi_grad(u; xi)
        // psi: R^nu --> R
        let psi = |u: &[f64], psi_val: &mut f64| -> Result<(), SolverError> {
            (alm_problem.parametric_cost)(u, &xi, psi_val)
        };
        // psi_grad: R^nu --> R^nu
        let psi_grad = |u: &[f64], psi_grad: &mut [f64]| -> Result<(), SolverError> {
            (alm_problem.parametric_gradient)(u, &xi, psi_grad)
        };
        // define the inner problem
        let inner_problem = Problem::new(&self.alm_problem.constraints, psi_grad, psi);
        // TODO: tolerance decrease until target tolerance is reached
        let mut inner_solver = PANOCOptimizer::new(inner_problem, &mut alm_cache.panoc_cache);
        // this method returns the result of .solve:
        inner_solver.solve(u)
    }

    fn is_exit_criterion_satisfied(&self) -> bool {
        let cache = &self.alm_cache;
        // Criterion 1: ||Delta y|| <= c * delta
        let criterion_1 = if let Some(xi) = &cache.xi {
            let c = xi[0];
            cache.delta_y_norm_plus <= c * self.delta_tolerance
        } else {
            true
        };
        // Criterion 2: ||F2(u+)|| <= delta
        let criterion_2 = cache.f2_norm_plus <= 1.0;
        criterion_1 && criterion_2
    }

    fn is_penalty_stall_criterion(&self) -> bool {
        let cache = &self.alm_cache;
        // Check whether the penalty parameter should not be updated
        // This is if iteration = 0, or there has been a sufficient
        // decrease in infeasibility
        if cache.iteration == 0
            || cache.delta_y_norm_plus < self.sufficient_decrease_coeff * cache.delta_y_norm
            || cache.f2_norm_plus < self.sufficient_decrease_coeff * cache.f2_norm
        {
            return true;
        }
        false
    }

    fn update_penalty(&mut self) {
        let cache = &mut self.alm_cache;
        if let Some(xi) = &mut cache.xi {
            xi[0] *= self.penalty_update_factor;
        }
    }

    fn update_inner_akkt_tolerance(&mut self) {
        let cache = &mut self.alm_cache;
        // epsilon_{nu+1} := max(epsilon, beta*epsilon_nu)
        cache.panoc_cache.set_akkt_tolerance(f64::max(
            cache.panoc_cache.akkt_tolerance.unwrap() * self.epsilon_update_factor,
            self.epsilon_tolerance,
        ));
    }

    fn final_cache_update(&mut self) {
        let cache = &mut self.alm_cache;
        cache.iteration += 1;
        cache.delta_y_norm = cache.delta_y_norm_plus;
        cache.f2_norm_plus = cache.f2_norm;
        cache.panoc_cache.reset();
    }
    /// Step of ALM algorithm
    fn step(&mut self, u: &mut [f64]) -> Result<bool, SolverError> {
        // Project y on Y
        self.project_on_set_y();
        // If the inner problem fails miserably, the failure should be propagated
        // upstream (using `?`). If the inner problem has not converged, that is fine,
        // we should keep solving.
        self.solve_inner_problem(u)
            .map(|_status: SolverStatus| {})?;
        // Update Lagrange multipliers:
        // y_plus <-- y + c*[F1(u_plus) - Proj_C(F1(u_plus) + y/c)]
        self.update_lagrange_multipliers(u)?;
        // Compute infeasibilities
        self.compute_pm_infeasibility(u)?;
        self.compute_alm_infeasibility()?;
        // Check exit criterion
        if self.is_exit_criterion_satisfied() {
            return Ok(false);
        } else if !self.is_penalty_stall_criterion() {
            self.update_penalty();
        }
        // Update inner problem tolerance
        self.update_inner_akkt_tolerance();
        // conclusive step: updated iteration count, resets PANOC cache,
        // sets f2_norm = f2_norm_plus etc
        self.final_cache_update();
        return Ok(true);
    }

    /* ---------------------------------------------------------------------------- */
    /*          MAIN API                                                            */
    /* ---------------------------------------------------------------------------- */

    /// Solve the specified ALM problem
    ///
    ///
    pub fn solve(&mut self, u: &mut [f64]) -> Result<(), SolverError> {
        // TODO: implement loop - check output of .step()
        let _step_result = self.step(u);
        Ok(())
    }
}

/* ---------------------------------------------------------------------------- */
/*          TESTS                                                               */
/* ---------------------------------------------------------------------------- */
#[cfg(test)]
mod tests {

    use crate::alm::*;
    use crate::core::constraints;
    use crate::core::panoc::*;
    use crate::SolverError;

    #[test]
    fn t_with_initial_penalty() {
        let tolerance = 1e-8;
        let nx = 10;
        let n1 = 5;
        let n2 = 0;
        let lbfgs_mem = 3;
        let panoc_cache = PANOCCache::new(nx, tolerance, lbfgs_mem);
        let mut alm_cache = AlmCache::new(panoc_cache, n1, n2);

        let f = |_u: &[f64], _p: &[f64], _cost: &mut f64| -> Result<(), SolverError> { Ok(()) };
        let df = |_u: &[f64], _p: &[f64], _grad: &mut [f64]| -> Result<(), SolverError> { Ok(()) };
        let f1 = |_u: &[f64], _grad: &mut [f64]| -> Result<(), SolverError> { Ok(()) };
        let set_c = constraints::Ball2::new(None, 1.50);

        let bounds = constraints::Ball2::new(None, 10.0);
        let set_y = constraints::Ball2::new(None, 1.0);
        let alm_problem = AlmProblem::new(
            bounds,
            Some(set_c),
            Some(set_y),
            f,
            df,
            Some(f1),
            NO_MAPPING,
            n1,
            n2,
        );

        let alm_optimizer =
            AlmOptimizer::new(&mut alm_cache, alm_problem).with_initial_penalty(7.0);
        assert!(!alm_optimizer.alm_cache.xi.is_none());
        if let Some(xi) = &alm_optimizer.alm_cache.xi {
            unit_test_utils::assert_nearly_equal(
                7.0,
                xi[0],
                1e-10,
                1e-12,
                "initial penalty parameter not set properly",
            );
        }

        let y_init = vec![2.0, 3.0, 4.0, 5.0, 6.0];
        let alm_optimizer = alm_optimizer.with_initial_lagrange_multipliers(&y_init);
        if let Some(xi) = &alm_optimizer.alm_cache.xi {
            unit_test_utils::assert_nearly_equal_array(
                &y_init,
                &xi[1..],
                1e-10,
                1e-12,
                "initial Langrange multipliers not set properly",
            );
        }
        // println!("cache = {:#?}", alm_optimizer.alm_cache);
    }
}
