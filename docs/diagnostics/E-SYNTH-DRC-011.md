# E-SYNTH-DRC-011 — connector mating face points inward

An edge-mounted connector's declared physical mating face does not point
outside the board. Courtyard clearance can pass while a USB-C or similar
receptacle is mechanically inaccessible.

The registry declares the unrotated footprint face using
`footprint_dimensions.mating_face`. The placer maps that face to the selected
board edge, and this DRC rule independently checks agent and sidecar rotation
overrides.

The suggested fix is to apply the reported rotation to the named connector.
