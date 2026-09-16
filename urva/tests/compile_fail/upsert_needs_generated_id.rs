use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "notes", id = String)]
pub struct Note {
    pub text: String,
}

use note_fields as n;

async fn go(notes: Store<Note>) -> Result<()> {
    notes
        .update_one(n::text.eq("x"), n::text.set("y"))
        .upsert()
        .await?;
    notes
        .find_one_and_update(n::_id.eq("a"), n::text.set("y"))
        .upsert()
        .await?;
    notes
        .bulk()
        .update_one_with(n::text.eq("x"), n::text.set("y"), |m| m.upsert = Some(true))
        .await?;
    notes.update_by_id("a", n::text.set("y")).upsert().await?;
    notes
        .find_one_and_update_by_id("a", n::text.set("y"))
        .upsert()
        .await?;
    Ok(())
}

fn main() {}
