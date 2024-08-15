//! The server side of the example.
//! It is possible (and recommended) to run the server in headless mode (without any rendering plugins).
//!
//! The server will:
//! - spawn a new player entity for each client that connects
//! - read inputs from the clients and move the player entities accordingly
//!
//! Lightyear will handle the replication of entities automatically if you add a `Replicate` component to them.
use crate::protocol::*;
use crate::shared;
use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::utils::HashMap;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use lightyear::shared::replication::components::ReplicationTarget;
use std::sync::Arc;
use std::time::Duration;

pub struct ExampleServerPlugin;

impl Plugin for ExampleServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (init, start_server));
        // the physics/FixedUpdates systems that consume inputs should be run in this set
        app.add_systems(FixedUpdate, movement);
        app.add_systems(
            Update,
            (transfer_authority, update_ball_color, handle_connections),
        );
    }
}

/// Start the server
fn start_server(mut commands: Commands) {
    commands.start_server();
}

/// Add some debugging text to the screen
fn init(mut commands: Commands) {
    commands.spawn(
        TextBundle::from_section(
            "Server",
            TextStyle {
                font_size: 30.0,
                color: Color::WHITE,
                ..default()
            },
        )
        .with_style(Style {
            align_self: AlignSelf::End,
            ..default()
        }),
    );
    commands.spawn((
        BallMarker,
        Name::new("Ball"),
        Position(Vec2::new(300.0, 0.0)),
        Speed(Vec2::new(0.0, 1.0)),
        PlayerColor(Color::WHITE),
        Replicate::default(),
    ));
}

/// Server connection system, create a player upon connection
pub(crate) fn handle_connections(
    mut connections: EventReader<ConnectEvent>,
    mut commands: Commands,
) {
    for connection in connections.read() {
        let client_id = connection.client_id;
        // server and client are running in the same app, no need to replicate to the local client
        let replicate = Replicate {
            sync: SyncTarget {
                prediction: NetworkTarget::Single(client_id),
                interpolation: NetworkTarget::AllExceptSingle(client_id),
            },
            controlled_by: ControlledBy {
                target: NetworkTarget::Single(client_id),
                ..default()
            },
            ..default()
        };
        let entity = commands.spawn((PlayerBundle::new(client_id, Vec2::ZERO), replicate));
        info!("Create entity {:?} for client {:?}", entity.id(), client_id);
    }
}

/// Handle client disconnections: we want to despawn every entity that was controlled by that client.
///
/// Lightyear creates one entity per client, which contains metadata associated with that client.
/// You can find that entity by calling `ConnectionManager::client_entity(client_id)`.
///
/// That client entity contains the `ControlledEntities` component, which is a set of entities that are controlled by that client.
///
/// By default, lightyear automatically despawns all the `ControlledEntities` when the client disconnects;
/// but in this example we will also do it manually to showcase how it can be done.
/// (however we don't actually run the system)
pub(crate) fn handle_disconnections(
    mut commands: Commands,
    mut disconnections: EventReader<DisconnectEvent>,
    manager: Res<ConnectionManager>,
    client_query: Query<&ControlledEntities>,
) {
    for disconnection in disconnections.read() {
        debug!("Client {:?} disconnected", disconnection.client_id);
        if let Ok(client_entity) = manager.client_entity(disconnection.client_id) {
            if let Ok(controlled_entities) = client_query.get(client_entity) {
                for entity in controlled_entities.entities() {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// Read client inputs and move players
pub(crate) fn movement(
    mut position_query: Query<(&ControlledBy, &mut Position), With<PlayerId>>,
    mut input_reader: EventReader<InputEvent<Inputs>>,
    tick_manager: Res<TickManager>,
) {
    for input in input_reader.read() {
        let client_id = input.context();
        if let Some(input) = input.input() {
            trace!(
                "Receiving input: {:?} from client: {:?} on tick: {:?}",
                input,
                client_id,
                tick_manager.tick()
            );
            // NOTE: you can define a mapping from client_id to entity_id to avoid iterating through all
            //  entities here
            for (controlled_by, position) in position_query.iter_mut() {
                if controlled_by.targets(client_id) {
                    shared::shared_movement_behaviour(position, input);
                }
            }
        }
    }
}

/// Assign authority over the ball to any player that comes close to it
pub(crate) fn transfer_authority(
    // timer so that we only transfer authority every X seconds
    mut timer: Local<Timer>,
    time: Res<Time>,
    mut commands: Commands,
    ball_q: Query<(Entity, &Position), With<BallMarker>>,
    player_q: Query<(&PlayerId, &Position)>,
) {
    if !timer.tick(time.delta()).finished() {
        return;
    }
    *timer = Timer::new(Duration::from_secs_f32(0.3), TimerMode::Once);
    for (ball_entity, ball_pos) in ball_q.iter() {
        // TODO: sort by player_id?
        for (player_id, player_pos) in player_q.iter() {
            if player_pos.0.distance(ball_pos.0) < 100.0 {
                trace!("Player {:?} has authority over the ball", player_id);
                commands
                    .entity(ball_entity)
                    .transfer_authority(AuthorityPeer::Client(player_id.0));
                return;
            }
        }

        // if no player is close to the ball, transfer authority back to the server
        commands
            .entity(ball_entity)
            .transfer_authority(AuthorityPeer::Server);
    }
}

/// Everytime the ball changes authority, repaint the ball according to the new owner
pub(crate) fn update_ball_color(
    mut balls: Query<
        (&mut PlayerColor, &AuthorityPeer),
        (With<BallMarker>, Changed<AuthorityPeer>),
    >,
    player_q: Query<(&PlayerId, &PlayerColor), Without<BallMarker>>,
) {
    for (mut ball_color, authority) in balls.iter_mut() {
        info!("Ball authority changed to {:?}", authority);
        match authority {
            AuthorityPeer::Server => {
                ball_color.0 = Color::WHITE;
            }
            AuthorityPeer::Client(client_id) => {
                for (player_id, player_color) in player_q.iter() {
                    if player_id.0 == *client_id {
                        info!("Set color client");
                        ball_color.0 = player_color.0;
                    }
                }
            }
            AuthorityPeer::None => {
                ball_color.0 = Color::BLACK;
            }
        }
    }
}
