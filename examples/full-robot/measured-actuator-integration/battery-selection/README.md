# Battery candidate and six-pound mass constraint

Checked September 15, 2026. The user requested a battery under $200 while keeping
the complete robot below six pounds (2.72155422 kg).

Candidate: [Gens ace 2200 mAh 3S 45C Air Classic, GEA223S45DGT](https://www.genstattu.com/gens-ace-2200mah-3s-45c-11-1v-g-tech-lipo-battery-pack-with-deans-plug/).
The manufacturer lists $35.09 before tax/shipping, 11.1 V nominal, 180 ±20 g,
106 ×34 ×23 mm, and a Deans connector. The normal 3S charged voltage is within
the HX-30HM's [specified 9–12.6 V operating range](https://www.hiwonder.com/products/hx-30hm).
Current delivery and sag with our motor harness still require validation; catalog
ratings do not identify battery internal resistance.

The current CAD export sums to **3.976239 kg / 8.7661 lb before a battery**.
With the candidate's nominal mass it would be **4.156239 kg / 9.1629 lb**.
Therefore no battery satisfies the six-pound limit with the present modeled mass.
Using 200 g as the pack allowance leaves 2.521554 kg / 5.5591 lb for everything else,
including electronics, wiring and mounting hardware.

This is an unresolved CAD mass budget, not a scale measurement. The existing
per-solid mass derivation uses CAD volume and material density; it does not apply
the displayed 30% print infill. Motor bodies use the manufacturer's 52 g value
and excluded internal geometry avoids counting that mass twice. Audit materials
and print mass/inertia derivations before changing physical parameters; do not
multiply all metal, motors and printed structure by one infill factor.

`candidate-and-mass-budget.json` records source hashes and calculations. The pack
has not been bought or selected automatically. The existing `battery-chain` gait
experiment remains a hypothetical 2 Ah electrical scenario and omits battery body
mass/inertia. A product-based scenario needs both electrical assumptions and an
explicit battery body/mount in CAD before it can predict the complete robot.
