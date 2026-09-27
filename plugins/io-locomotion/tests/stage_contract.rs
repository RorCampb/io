//! A developer's actions/properties adapt into the supplied stages, without engine edits.
use io_game::stage::Stage;
use io_locomotion::pipeline::{ActorClearanceStage, SupportStage, WalkError, WalkRequest};
use io_traversal::navigation::Waypoint;
use io_types::Vec3;
use io_world::*;

#[derive(Clone, Copy)]
enum Action {
    Advance,
    Sneak,
}
struct Properties {
    standing: f32,
    sneaking: f32,
}
struct Request<'a> {
    world: &'a dyn WorldView,
    route_step: Waypoint<Action>,
    properties: &'a Properties,
    scratch: &'a mut Vec<Vec3>,
}
struct Prepare;
impl<'a> Stage<Request<'a>> for Prepare {
    type Output = WalkRequest<'a>;
    type Error = WalkError;
    fn run(&mut self, input: Request<'a>) -> Result<Self::Output, Self::Error> {
        let start = input.world.item(99).unwrap().transform.anchor;
        let height = match input.route_step.action {
            Action::Advance => input.properties.standing,
            Action::Sneak => input.properties.sneaking,
        };
        let target = input.route_step.position;
        Ok(WalkRequest {
            world: input.world,
            actor: Some(99),
            start,
            delta: Vec3::new(target.x - start.x, target.y - start.y, 0.),
            shape: CharacterBody {
                radius: 0.35,
                height,
                max_slope: 0.8,
            },
            scratch: input.scratch,
        })
    }
}

#[test]
fn custom_route_action_and_properties_feed_the_existing_pipeline() {
    let box_item = |id, position, half_extents| Item {
        id,
        transform: Transform::new(position, Vec3::new(1., 1., 1.), 0.).unwrap(),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box { half_extents })),
        ..Default::default()
    };
    let world = World::new(
        Space::new(Vec3::new(20., 20., 10.)),
        vec![
            box_item(1, Vec3::new(0., 0., -0.5), Vec3::new(5., 5., 0.5)),
            box_item(2, Vec3::new(2., 0., 1.8), Vec3::new(0.5, 2., 0.3)),
            Item {
                id: 99,
                character_body: Some(CharacterBody {
                    radius: 0.35,
                    height: 1.9,
                    max_slope: 0.8,
                }),
                ..Default::default()
            },
        ],
    );
    let properties = Properties {
        standing: 1.9,
        sneaking: 1.1,
    };
    let mut scratch = vec![];
    let mut pipeline = Prepare.then(SupportStage).then(ActorClearanceStage);
    for (action, reaches) in [(Action::Advance, false), (Action::Sneak, true)] {
        let result = pipeline
            .run(Request {
                world: &world,
                properties: &properties,
                route_step: Waypoint {
                    position: Vec3::new(3.5, 0., 0.),
                    action,
                },
                scratch: &mut scratch,
            })
            .unwrap()
            .into_walk();
        assert_eq!(result.reached, reaches);
    }
    assert_eq!(
        world.item(99).unwrap().transform.anchor,
        Vec3::default(),
        "stages do not mutate the world"
    );
}
