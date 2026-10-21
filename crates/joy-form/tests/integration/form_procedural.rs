// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Procedural geometry: aliasing and snapshots, attributes, combining, retained instances, generators, and wrangle
//! rules.

use joy_form::{
    AttributeColumn,
    compile,
};
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
    assert_eq!(outputs[0].value.json()["attributes"]["points"]["weight"]["values"], json!([3.0]));
    assert_eq!(outputs[1].value.json()["attributes"]["points"]["weight"]["values"], json!([4.0]));
    assert_eq!(outputs[2].value.json()["attributes"]["points"]["weight"]["values"], json!([8.0, 0.0]));
    assert_eq!(result.positions, vec![0.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
}

#[test]
fn corner_attributes_split_render_vertices_and_reflections_preserve_alignment() {
    let result = compile(
        r#"
        let g=grid(1,1,2,2)
        addAttribute(g,"corners","color",rgba(1,0,0,1))
        wrangle corners in g as c { if c.index==0 { c.color=rgba(0,0,3,1) } }
        scale(g,vector(-1,1,1))
        output("Grid",g)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.positions.len(), 18);
    // Color attributes are linear and keep HDR channels above one.
    assert_eq!(&result.colors[..3], &[0.0, 0.0, 3.0]);
    assert_eq!(result.triangles, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn retained_instances_own_sites_and_material_and_carry_site_attributes() {
    let result = compile(
        r#"
        let sites=geometry()
        addPoint(sites,vector(1,0,0),{kind:"marker",id:4})
        let collection=instances(meshBox(1,1,1),sites,materialPbr(rgba(1,1,1,1),0,0.5))
        addAttribute(collection,"instances","scale",vector(1,1,1))
        wrangle instances in collection as item { item.scale=vector(2,1,1) }
        translate(sites,vector(100,0,0))
        output("Assembly",collection)
        output("Realized",realize(collection))
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.positions[0], 0.0);
    assert_eq!(result.materials.len(), 2);
    assert_eq!(
        result.vertex_attributes["kind"],
        AttributeColumn::Text {
            values: vec!["marker".into(); 16]
        }
    );
    assert_eq!(
        result.vertex_attributes["id"],
        AttributeColumn::Number {
            size: 1,
            values: vec![4.0; 16]
        }
    );
    assert_eq!(
        result.outputs.as_ref().unwrap()[0].value.json()["sites"]["attributes"]["points"]["position"]["values"],
        json!([[1.0, 0.0, 0.0]])
    );
    // Realizing a collection into geometry gives the same vertices, colors, and materials as drawing it.
    assert_eq!(&result.positions[..24], &result.positions[24..]);
    assert_eq!(&result.colors[..24], &result.colors[24..]);
    assert_eq!(&result.triangle_materials[..12], &result.triangle_materials[12..]);
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
        let after=geometry()
        addPoint(after,vector(rand(),0,0))
        output("After",after)
    "#;
    let result = compile(source, &json!({}), None).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(compile(source, &json!({}), None).unwrap()).unwrap()
    );
    let outputs = result.outputs.unwrap();
    assert_eq!(outputs[0].value.json()["faces"].as_array().unwrap().len(), 16);
    assert_eq!(outputs[0].geometry.positions.len(), 144);
    assert_eq!(outputs[2].value.json()["attributes"]["points"]["position"]["values"].as_array().unwrap().len(), 5);
    assert_eq!(outputs[3].geometry.points, vec![0]);
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

#[test]
fn merging_keeps_each_part_looking_the_same_and_unions_attributes() {
    let result = compile(
        r#"
        let painted = color(box(1,1,1), rgba(0,1,0,1))
        let plain = translate(box(1,1,1), vector(3,0,0))
        let striped = translate(grid(1,1,1,1), vector(6,0,0))
        addAttribute(striped,"corners","color",rgba(1,0,0,1))
        addAttribute(plain,"faces","chunk",2)
        addAttribute(painted,"points","label","painted")
        material(plain, materialPbr(rgba(0.5,0.5,0.5,1),0,0.4))
        let parts = merge(painted, plain)
        append(parts, striped)
        output("Parts", parts)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    // Corner colors on one part move every part's colors to corners, so each keeps its own color.
    let colors: Vec<[f64; 3]> = result.colors.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    assert_eq!(colors[0], [0.0, 1.0, 0.0]);
    assert_eq!(colors[36], [0.929, 0.424, 0.224]);
    assert_eq!(colors[72], [1.0, 0.0, 0.0]);
    // Custom attributes absent from a part take their defaults there.
    assert_eq!(result.triangle_record(0)["chunk"], 0.0);
    assert_eq!(result.triangle_record(12)["chunk"], 2.0);
    assert_eq!(result.vertex_record(0)["label"], "painted");
    assert_eq!(result.vertex_record(36)["label"], "");
    // Only the plain box carries the script material.
    assert_eq!((result.triangle_materials[0], result.triangle_materials[12], result.triangle_materials[24]), (0, 1, 0));

    let error = compile(
        "let a = box(1,1,1) addAttribute(a,\"points\",\"tag\",1) let b = box(1,1,1) addAttribute(b,\"points\",\"tag\",\"x\") let c = merge(a, b)",
        &json!({}),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.message,
        "Cannot combine points attribute 'tag': it is a number in one geometry and text in the other."
    );
}

#[test]
fn text_true_false_and_material_attributes_read_and_write_in_wrangles() {
    let result = compile(
        r#"
        let steel = materialPbr(rgba(0.6,0.6,0.65,1),1,0.3)
        let g = grid(2,1,2,1)
        addAttribute(g,"faces","material",null)
        addAttribute(g,"faces","zone","street")
        addAttribute(g,"points","lit",false)
        wrangle faces in g as face {
            if face.index % 2 == 0 { face.material = steel face.zone = "roof" }
        }
        wrangle points in g as point { point.lit = point.position.x > 0 }
        let first = attribute(g,"faces","zone",0)
        let roofs = 0
        if first == "roof" AND hasAttribute(g,"faces","material") { roofs = 1 }
        addPoint(g, vector(9, roofs, 0))
        output("Grid", g)
    "#,
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.triangle_materials, vec![1, 0, 1, 0]);
    assert_eq!(
        result.triangle_attributes["zone"],
        AttributeColumn::Text {
            values: vec!["roof".into(), "street".into(), "roof".into(), "street".into()]
        }
    );
    assert_eq!(result.vertex_record(2)["lit"], true);
    assert_eq!(result.vertex_record(0)["lit"], false);
    let last = result.positions.len() - 3;
    assert_eq!(result.positions[last + 1], 1.0);

    let error = compile(
        "let g=grid(1,1,1,1) addAttribute(g,\"points\",\"tag\",1) setAttribute(g,\"points\",\"tag\",0,true)",
        &json!({}),
        None,
    )
    .unwrap_err();
    assert_eq!(error.message, "Attribute 'tag' must keep its type (a number).");
}
