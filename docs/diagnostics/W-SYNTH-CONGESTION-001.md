# W-SYNTH-CONGESTION-001: Predicted Routing Bottleneck Zone

## Severity
`Severity::Warning`

## Cause
The GNN spatial congestion advisor detected a high-density spatial bottleneck zone ($\text{probability} > 0.85$) during net placement and A* routing evaluation. Traces were forced through a narrow channel between component courtyards, which may increase trace length or restrict signal integrity.

## Explanation
Synth's Graph Convolutional Network (GCN) congestion predictor maps the component hypergraph connectivity and pin density to a 2D spatial grid. When components are tightly clustered, the router's search space is constrained, triggering cost multiplier scaling $g(u,v) = \text{dist}(u,v) \cdot (1.0 + \alpha \cdot \mathbf{C}(x,y))$.

## Suggested Fix
1. Increase component spacing around the flagged location to widen the routing channel.
2. Use placement hints (`placement_hint`) to nudge macro components away from the high-density pin cluster.
3. Re-run `synth route` to allow the GNN advisor to re-evaluate the unweighted spatial field.
