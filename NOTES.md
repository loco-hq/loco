Next things to try:

1. Let's work on cleaning up the schemas. I'm still not sure if site and dataset names are being correctly qualified with their projects.

2. Let's think about loco.yaml and how it's currely a "special" thing. Let's investigate how we could make it just another type just like anything else. Also, there seems to be some loco-apps specific code in loco-gen about namespaces/projects, etc. Could we separate those concepts into loco-apps? Here's what I'm thinking... loco-gen is just a type definition layer and runtime cache that can load in data from a filesystem or any other "source adapter" as we make them. loco-gen should not know anything about the meaning of any of the metadata or what it does. That is the job of an application that is making use of loco-gen functionality, like loco-apps. Let's discuss this, and possibly make a plan to
   a. make the loco.yaml files just another type, like maybe "manifest". In order to store an array of the dependencies though, we'll need to improve loco-gen to be able to handle a lists/sequences from a yaml file. This will need to be another type, like string, number, boolean, etc.
   b. move any functionality that interprets meaning from the manifest file into loco-apps.

Does all this make sense. Please push back on this if you see any problems with this separation, or have any caviats to add for me to think through.

3. Maybe we should be able to edit the labels/descriptions of projects in the cards app.

4. Enforce create-only properties and write tests.

5. It looks like users aren't stored in the data-lake. I'm wonder if we could adjust so that those are actually stored in the lake.

6. It looks like the auth functionality isn't using the x-site id header like the other functionality is. We should switch to doing that.

7. For loco-lake, let's put the individual adapters into an "adapters" folder.
