let assembly = cad("assembly");
let p = parameters();
configure(#{
    expectations: [
        #{name: "Settled first-joint tracking", signal: "joints/hinge1/angle",
          unit: "rad", reduction: "rmse", target: p.target1, start: 0.6, max: 0.05},
        #{name: "First motor peak current", signal: "motors/drive1/current",
          unit: "A", reduction: "max_abs", max: 2.5}
    ]
});
