Product Requirements Document for Autonomi Network Enhancements
Introduction


This document outlines the product requirements for the development and enhancement of the Autonomi Network (formerly known as the MaidSafe Safe Network). The Autonomi Network is a fully decentralized platform aimed at providing secure, private, and efficient data storage and communication. This document details the necessary work to implement and improve various aspects of the network, including data types, client APIs, network architecture, and payment systems.


Objectives


 • Implement and document four core data types essential for network operations.
 • Enhance the network’s decentralization by refining bootstrap mechanisms.
 • Define and standardize client API behaviors in a decentralized environment.
 • Ensure the client API comprehensively documents all data types.
 • Restrict store/get methods to accept only the defined data types.
 • Integrate a flexible payment system utilizing EVM and L2 networks with runtime configurability.


1. Data Types


The Autonomi Network will support four primary data types:


1.1 Chunks


 • Description: Immutable data pieces up to 1 MB in size.
 • Naming Convention: The name of a chunk is derived from the hash of its content (hash(content) == name).
 • Purpose: Enables content-addressable storage, ensuring data integrity and deduplication.


1.2 Registers


 • Description: Conflict-free Replicated Data Type (CRDT) directed acyclic graphs (DAGs).
 • Concurrency Handling: Allows multiple concurrent accesses. In cases of conflicting updates, users are responsible for merging changes, as the network does not handle conflict resolution.
 • Use Case: Suitable for collaborative applications where eventual consistency is acceptable.


1.3 Transactions


 • Description: Simple data structures representing value transfers.
 • Structure:
 • Owner: Identified by a public key.
 • Content: May include a value and an optional additional key.
 • Outputs: A set of keys indicating recipients of the transaction.
 • Validation: Clients must verify the transaction history to ensure correctness.
 • Purpose: Facilitates decentralized transactions without central authority oversight.


1.4 Vault


 • Description: Flexible data type up to 1 MB that can encapsulate any developer-defined data structure.
 • Ownership: Secured by an owner’s public key.
 • Versioning:
 • Not a CRDT.
 • Includes a user or application-defined counter.
 • Nodes retain only the copy with the highest counter value after signature verification.
 • Use Case: Ideal for applications requiring custom data storage with version control.


2. Network Architecture


2.1 Decentralization


 • The network operates without central servers, promoting resilience and autonomy.
 • Bootstrap nodes exist solely for initial network access.


2.2 Bootstrap Nodes


 • Purpose: Aid first-time nodes or clients in connecting to the network.
 • Limitations:
 • Must not be relied upon for continued operation.
 • Designed to be ephemeral and can disappear without affecting the network.
 • Distribution:
 • New bootstrap nodes can be published via websites, DNS records, or shared among users.
 • Users are encouraged to share bootstrap information to foster decentralization.


2.3 Bootstrap Cache


 • Functionality:
 • Nodes and clients must collect and maintain their own network contacts after the initial connection.
 • This cache is used for reconnecting to the network autonomously.
 • Benefit: Eliminates dependence on specific bootstrap nodes, enhancing network robustness.


3. Client API


3.1 Connection Model


 • Stateless Connectivity:
 • Clients acknowledge that persistent connections are impractical in a decentralized network unless designed to receive unsolicited messages.
(i.e. the client.connect() does not make sense in our current situation.)
 • Operational Behavior:
 • Clients maintain a list of network addresses.
 • For any action, they connect to the nearest node and discover nodes closest to the target address.
 • Addresses collected during operations are stored in the bootstrap cache.


3.2 Data Types Definition


 • Centralized Documentation:
 • All four data types must be clearly defined and documented within a single section of the API documentation.
 • Developer Guidance:
 • Provide detailed explanations, usage examples, and best practices for each data type.


3.3 Store/Get Methods


 • Data Type Restrictions:
 • The API’s store/get methods are configured to accept only the four defined data types.
 • Inputs of other data types are explicitly disallowed to maintain data integrity and consistency.


4. Payment System Integration


4.1 EVM and L2 Network Utilization


 • Blockchain Integration:
 • Leverage the Ethereum Virtual Machine (EVM) and Layer 2 (L2) networks for transaction processing.
 • Runtime Configurability:
 • Nodes and clients can modify payment-related settings at runtime.
 • Configurable parameters include wallet details, chosen payment networks, and other relevant settings.


4.2 Wallet Management


 • Flexibility:
 • Users can change wallets without restarting or recompiling the client or node software.
 • Security:
 • Ensure secure handling and storage of wallet credentials and transaction data.


5. Additional Requirements


 • Scalability: Design systems to handle network growth without performance degradation.
 • Security: Implement robust encryption and authentication mechanisms across all components.
 • Performance: Optimize data storage and retrieval processes for efficiency.
 • Usability: Provide clear documentation and intuitive interfaces for developers and end-users.


6. Documentation and Support


 • Comprehensive Guides:
 • Produce detailed documentation for all new features and changes.
 • Include API references, tutorials, and FAQs.
 • Community Engagement:
 • Encourage community feedback and contributions.
 • Provide support channels for troubleshooting and discussions.


Conclusion


Implementing these requirements will enhance the Autonomi Network’s functionality, security, and user experience. Focusing on decentralization, flexibility, and clear documentation will position the network as a robust platform for decentralized applications and services.
