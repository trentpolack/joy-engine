// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Procedural geometry: aliasing and snapshots, attributes, retained instances, generators, and wrangle rules.

use joy_form::compile;
use serde_json::json;

#[test]
fn aliases_copies_output_snapshots_and_new_attribute_defaults_are_distinct() {
    let result = compile(
        r#"
        let g = geometry()
        addAttribute(g,"points","weight",7)
        addPoint(g,vector(0,0,0))
        let alias = g
        setAttribute(alias,"points","weight",0,3)
        let duplicate = copy(g)
        output("Original",g)
        wrangle points in duplicate as p { p.position = p.position + vector(0,2,0) p.weight=4 }
        output("Copy",duplicate)
        setAttribute(g,"points","weight",0,8)
        addPoint(g,vector(1,0,0))
        inspect("After",g)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    let outputs = result.outputs.unwrap();
    assert_eq!(outputs[0]["value"]["attributes"]["points"]["weight"]["values"], json!([3.0]));
    assert_eq!(outputs[1]["value"]["attributes"]["points"]["weight"]["values"], json!([4.0]));
    assert_eq!(outputs[2]["value"]["attributes"]["points"]["weight"]["values"], json!([8.0, 0.0]));
    assert_eq!(result.positions, vec![0.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
}

#[test]
fn corner_attributes_split_render_vertices_and_reflections_preserve_alignment() {
    let result = compile(
        r#"
        color(1,1,1)
        let g=grid(1,1,2,2)
        addAttribute(g,"corners","color",rgba(1,0,0,1))
        wrangle corners in g as c { if c.index==0 { c.color=rgba(0,0,1,1) } }
        scale(g,vector(-1,1,1))
        output("Grid",g)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.positions.len(), 18);
    assert_eq!(&result.colors[..3], &[0.0, 0.0, 1.0]);
    assert_eq!(result.triangles, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn retained_instances_own_sites_and_material_and_preserve_metadata() {
    let result = compile(
        r#"
        color(1,1,1)
        let sites=geometry()
        addPoint(sites,vector(1,0,0))
        setUserData(sites,0,{kind:"marker",id:4})
        let collection=instances(meshBox(1,1,1),sites,materialPbr(rgba(1,1,1,1),0,0.5))
        addAttribute(collection,"instances","scale",vector(1,1,1))
        wrangle instances in collection as item { item.scale=vector(2,1,1) }
        translate(sites,vector(100,0,0))
        output("Assembly",collection)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.positions[0], 0.0);
    assert_eq!(result.materials.len(), 2);
    assert!(result.user_data.as_ref().unwrap().iter().all(|data| data == &json!({"kind":"marker","id":4.0})));
    assert_eq!(
        result.outputs.as_ref().unwrap()[0]["value"]["sites"]["attributes"]["points"]["position"]["values"],
        json!([[1.0, 0.0, 0.0]])
    );
}

#[test]
fn generators_preserve_uv_seams_normals_and_local_scatter_randomness() {
    let source = r#"
        let profile=geometry()
        addPoint(profile,vector(1,0,0)) addPoint(profile,vector(1,2,0))
        output("Vessel",revolve(profile,8))
        let path=geometry()
        addPoint(path,vector(0,0,0)) addPoint(path,vector(0,2,0)) addPoint(path,vector(1,3,0))
        output("Pipe",tube(path,0.25,8))
        let grid=grid(1,1,2,2)
        addAttribute(grid,"points","density",1)
        seed(42)
        let sites=scatter(grid,5,7777,"density")
        inspect("Sites",sites)
        point(rand(),0,0)
    "#;
    let result = compile(source, &json!({}), None).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(compile(source, &json!({}), None).unwrap()).unwrap()
    );
    let outputs = result.outputs.unwrap();
    assert_eq!(outputs[0]["value"]["faces"].as_array().unwrap().len(), 16);
    assert_eq!(outputs[0]["geometry"]["positions"].as_array().unwrap().len(), 144);
    assert_eq!(outputs[2]["value"]["attributes"]["points"]["position"]["values"].as_array().unwrap().len(), 5);
    assert!((result.positions[result.positions.len() - 3] - 0.2523451747838408).abs() < 1e-15);
}

#[test]
fn illegal_wrangle_mutations_and_budget_bypasses_reject_with_locations() {
    for source in [
        "let g=grid(1,1,2,2) wrangle points in g as p { p.missing=1 }",
        "let g=grid(1,1,2,2) wrangle points in g as p { addPoint(g,vector(0,0,0)) }",
        "let g=grid(1,1,2,2) wrangle points in g as p { wrangle points in g as q {} }",
        "let g=grid(256,256,2,2) let a=copy(g) let b=copy(g)",
    ] {
        let error = compile(source, &json!({}), None).unwrap_err();
        assert_eq!(error.line, 1);
    }
}

#[test]
fn overflowing_normal_transforms_reject_at_the_operation_in_every_domain() {
    for domain in ["points", "faces", "corners"] {
        for (normal, operation) in [
            ("vector(1e308,1e308,0)", "scale(g,vector(2,2,2))"),
            ("vector(1.7e308,1.7e308,0)", "rotate(g,vector(0,0,pi/4))"),
        ] {
            let source = format!(
                "let g=geometry()\naddPoint(g,vector(0,0,0))\naddFace(g,0,0,0)\naddAttribute(g,\"{domain}\",\"normal\",{normal})\n  {operation}\noutput(\"g\",g)"
            );
            let error = compile(&source, &json!({}), None)
                .expect_err("a computed nonfinite normal must not become a zero sentinel");
            assert_eq!(error.message, "Transformed normal components must be finite.");
            assert_eq!((error.line, error.column), (5, 3));
        }
    }
}

#[test]
fn normal_transforms_preserve_zero_sentinels_and_large_finite_directions() {
    for domain in ["points", "faces", "corners"] {
        for (normal, expected) in [("vector(0,0,0)", vec![0.0, 0.0, 0.0]), ("vector(1e307,0,0)", vec![1.0, 0.0, 0.0])] {
            let source = format!(
                "let g=geometry() addPoint(g,vector(0,0,0)) addFace(g,0,0,0) addAttribute(g,\"{domain}\",\"normal\",{normal}) scale(g,vector(2,2,2)) rotate(g,vector(0,0,0)) output(\"g\",g)"
            );
            let result = compile(&source, &json!({}), None).unwrap();
            assert!(!result.normals.is_empty());
            for normal in result.normals.as_chunks::<3>().0 {
                assert_eq!(normal.as_slice(), expected.as_slice());
            }
        }
    }
}

#[test]
fn overflowing_normal_lengths_reject_at_the_operation_in_every_domain() {
    for domain in ["points", "faces", "corners"] {
        let source = format!(
            "let g=geometry()\naddPoint(g,vector(0,0,0))\naddFace(g,0,0,0)\naddAttribute(g,\"{domain}\",\"normal\",vector(1.7e308,1.7e308,1.7e308))\n  rotate(g,vector(0,0,0))\noutput(\"g\",g)"
        );
        let error =
            compile(&source, &json!({}), None).expect_err("an overflowed normal length must not produce zero normals");
        assert_eq!(error.message, "Transformed normal length must be finite.");
        assert_eq!((error.line, error.column), (5, 3));
    }
}
